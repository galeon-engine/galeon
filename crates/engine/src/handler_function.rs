// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

use std::marker::PhantomData;

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::system_param::{Access, SystemParam};
use crate::world::{UnsafeWorldCell, World};

// =============================================================================
// Handler trait — trait-object interface for all handler types
// =============================================================================

/// Trait-object interface for all handler types.
///
/// Parallel to [`System`][crate::function_system::System], but shaped for
/// request/response invocation: the caller supplies a `Req` value and receives
/// `Result<Resp, String>` back. SystemParams are injected from the ECS world
/// just as they are in regular systems.
pub trait Handler<Req, Resp> {
    /// Human-readable handler name (for diagnostics and conflict messages).
    fn name(&self) -> &'static str;

    /// Run the handler with the given request against the world.
    fn run(&mut self, request: Req, world: &mut World) -> Result<Resp, String>;

    /// Declare what world data this handler accesses.
    ///
    /// Returns the union of all parameter accesses for parameterized handlers.
    fn access(&self) -> Vec<Access>;
}

// =============================================================================
// IntoHandler — converts a compatible function into a boxed Handler
// =============================================================================

/// Converts a compatible function into a boxed [`Handler`].
///
/// Implemented for parameterized functions `fn(Req, P0, P1, ...) -> Result<Resp, E>`
/// where each `P` is a [`SystemParam`] and `E: ToString`. The error is
/// converted to `String` at the bridge boundary. Parallel to
/// [`IntoSystem`][crate::function_system::IntoSystem].
pub trait IntoHandler<Req, Resp, Params> {
    fn into_handler(self, name: &'static str) -> Box<dyn Handler<Req, Resp>>;
}

// =============================================================================
// Parameterized handler — fn(Req, P0, P1, ...) where each P: SystemParam
// =============================================================================

/// Internal bridge trait. Parallel to `SystemParamFunction`.
///
/// Bridges an `FnMut(Req, P::Item<'_>, ...) -> Result<Resp, E>` (where
/// `E: ToString`) to the `Handler::run` interface, converting errors to
/// `String` at the boundary.
pub(crate) trait HandlerParamFunction<Req, Resp, Params>: 'static {
    fn run(&mut self, request: Req, world: &mut World) -> Result<Resp, String>;
    fn param_access() -> Vec<Access>;
}

/// Wraps a [`HandlerParamFunction`] into a [`Handler`] trait object.
/// Parallel to `ParamSystem`.
struct ParamHandler<F, Req, Resp, Params> {
    name: &'static str,
    func: F,
    _marker: PhantomData<fn(Req) -> (Resp, Params)>,
}

impl<F, Req, Resp, Params> Handler<Req, Resp> for ParamHandler<F, Req, Resp, Params>
where
    F: HandlerParamFunction<Req, Resp, Params>,
{
    fn name(&self) -> &'static str {
        self.name
    }

    fn run(&mut self, request: Req, world: &mut World) -> Result<Resp, String> {
        self.func.run(request, world)
    }

    fn access(&self) -> Vec<Access> {
        F::param_access()
    }
}

// =============================================================================
// Conflict validation (T2)
// =============================================================================

/// Panics if any two accesses within the same handler conflict.
fn validate_no_self_conflicts(access: &[Access], handler_name: &'static str) {
    for (i, a) in access.iter().enumerate() {
        for b in &access[i + 1..] {
            if a.conflicts_with(b) {
                panic!(
                    "handler '{}' has conflicting parameter access: {:?} vs {:?}",
                    handler_name, a, b,
                );
            }
        }
    }
}

// =============================================================================
// run_handler — transport-neutral invocation entrypoint (T3)
// =============================================================================

/// Execute a handler against the world with the given request.
///
/// This is the transport-neutral invocation entrypoint. Transport adapters
/// (axum routes, WASM bridge, etc.) call this to run a handler function
/// with ECS parameter injection.
///
/// # Example
///
/// ```rust,ignore
/// let mut handler = (|req: MyRequest| Ok(MyResponse { ok: true }))
///     .into_handler("my_handler");
/// let result = run_handler(&mut *handler, MyRequest { id: 1 }, &mut world);
/// ```
pub fn run_handler<Req, Resp>(
    handler: &mut dyn Handler<Req, Resp>,
    request: Req,
    world: &mut World,
) -> Result<Resp, String> {
    handler.run(request, world)
}

// =============================================================================
// JSON boundary — serde at the transport edge (#173)
// =============================================================================

/// Deserialize JSON, run a [`Handler`], serialize the response as JSON.
///
/// Transport adapters (for example generated axum routes) use this at the
/// HTTP boundary while keeping handler execution on [`World`] via
/// [`run_handler`].
pub fn run_json_handler<Req, Resp>(
    handler: &mut dyn Handler<Req, Resp>,
    json_body: &str,
    world: &mut World,
) -> Result<String, String>
where
    Req: DeserializeOwned,
    Resp: Serialize,
{
    let request: Req = serde_json::from_str(json_body).map_err(|e| e.to_string())?;
    let response = run_handler(handler, request, world)?;
    serde_json::to_string(&response).map_err(|e| e.to_string())
}

/// Deserialize JSON, run a [`Handler`], and return the response as [`serde_json::Value`].
///
/// Same as [`run_json_handler`], but avoids a serialize-then-parse round trip
/// when the transport layer needs a JSON value (for example axum `Json<Value>`).
pub fn run_json_handler_value<Req, Resp>(
    handler: &mut dyn Handler<Req, Resp>,
    json_body: &str,
    world: &mut World,
) -> Result<serde_json::Value, String>
where
    Req: DeserializeOwned,
    Resp: Serialize,
{
    let request: Req = serde_json::from_str(json_body).map_err(|e| e.to_string())?;
    let response = run_handler(handler, request, world)?;
    serde_json::to_value(&response).map_err(|e| e.to_string())
}

/// JSON boundary helper for any function that implements [`IntoHandler`].
///
/// Builds a fresh boxed handler from `f` each call (same cost model as
/// per-request `into_handler` in generated glue).
///
/// For handlers with extra [`SystemParam`] arguments, Rust may fail to infer
/// the `Params` type tuple when this helper is called indirectly. Prefer
/// [`IntoHandler::into_handler`] plus [`run_json_handler`] or
/// [`run_json_handler_value`] in a scoped function (generated axum glue uses
/// this pattern).
pub fn run_json_handler_function<F, Req, Resp, Params>(
    f: F,
    handler_name: &'static str,
    json_body: &str,
    world: &mut World,
) -> Result<String, String>
where
    F: IntoHandler<Req, Resp, Params>,
    Req: DeserializeOwned + 'static,
    Resp: Serialize + 'static,
{
    let mut handler = f.into_handler(handler_name);
    run_json_handler(&mut *handler, json_body, world)
}

// =============================================================================
// Zero-param impl — fn(Req) -> Result<Resp, E> where E: ToString
// =============================================================================

impl<Func, Req, Resp, Err> HandlerParamFunction<Req, Resp, ()> for Func
where
    Func: FnMut(Req) -> Result<Resp, Err> + 'static,
    Req: 'static,
    Resp: 'static,
    Err: ToString + 'static,
{
    fn run(&mut self, request: Req, _world: &mut World) -> Result<Resp, String> {
        self(request).map_err(|e| e.to_string())
    }

    fn param_access() -> Vec<Access> {
        Vec::new()
    }
}

impl<Func, Req, Resp> IntoHandler<Req, Resp, ()> for Func
where
    Func: HandlerParamFunction<Req, Resp, ()>,
    Req: 'static,
    Resp: 'static,
{
    fn into_handler(self, name: &'static str) -> Box<dyn Handler<Req, Resp>> {
        Box::new(ParamHandler {
            name,
            func: self,
            _marker: PhantomData,
        })
    }
}

// =============================================================================
// Arity macros — 1..8 parameter handlers
// =============================================================================

macro_rules! impl_handler_param_function {
    ($($P:ident),+) => {
        #[allow(non_snake_case)]
        impl<Func, Req, Resp, Err, $($P: SystemParam + 'static),+> HandlerParamFunction<Req, Resp, ($($P,)+)> for Func
        where
            Func: FnMut(Req, $($P::Item<'_>),+) -> Result<Resp, Err> + 'static,
            Req: 'static,
            Resp: 'static,
            Err: ToString + 'static,
        {
            fn run(&mut self, request: Req, world: &mut World) -> Result<Resp, String> {
                // SAFETY: same justification as SystemParamFunction — conflict
                // detection at registration ensures no two params access the
                // same TypeId mutably. UnsafeWorldCell provides field-level
                // access via addr_of!, so fetch() impls never create
                // intermediate &World / &mut World references — only
                // field-level references to `resources` or `archetypes`,
                // which live in separate memory regions.
                let cell = unsafe { UnsafeWorldCell::new(world as *mut World) };
                unsafe { self(request, $($P::fetch(cell),)+) }.map_err(|e| e.to_string())
            }

            fn param_access() -> Vec<Access> {
                let mut acc = Vec::new();
                $(acc.extend($P::access());)+
                acc
            }
        }

        impl<Func, Req, Resp, $($P: SystemParam + 'static),+> IntoHandler<Req, Resp, ($($P,)+)> for Func
        where
            Func: HandlerParamFunction<Req, Resp, ($($P,)+)>,
            Req: 'static,
            Resp: 'static,
        {
            fn into_handler(self, name: &'static str) -> Box<dyn Handler<Req, Resp>> {
                let access = <Func as HandlerParamFunction<Req, Resp, ($($P,)+)>>::param_access();
                validate_no_self_conflicts(&access, name);
                Box::new(ParamHandler {
                    name,
                    func: self,
                    _marker: PhantomData,
                })
            }
        }
    };
}

impl_handler_param_function!(P0);
impl_handler_param_function!(P0, P1);
impl_handler_param_function!(P0, P1, P2);
impl_handler_param_function!(P0, P1, P2, P3);
impl_handler_param_function!(P0, P1, P2, P3, P4);
impl_handler_param_function!(P0, P1, P2, P3, P4, P5);
impl_handler_param_function!(P0, P1, P2, P3, P4, P5, P6);
impl_handler_param_function!(P0, P1, P2, P3, P4, P5, P6, P7);

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::component::Component;
    use crate::system_param::{Query, QueryMut, Res, ResMut};
    use serde::{Deserialize, Serialize};

    // -------------------------------------------------------------------------
    // Test types
    // -------------------------------------------------------------------------

    #[derive(Debug)]
    struct Counter(u32);
    impl Component for Counter {}

    struct Config {
        multiplier: f32,
    }

    #[derive(Debug, Deserialize)]
    struct SpawnRequest {
        unit_id: u64,
    }

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct SpawnResponse {
        ok: bool,
    }

    // -------------------------------------------------------------------------
    // T4: Positive execution tests
    // -------------------------------------------------------------------------

    fn spawn_no_params(req: SpawnRequest) -> Result<SpawnResponse, String> {
        Ok(SpawnResponse {
            ok: req.unit_id > 0,
        })
    }

    #[test]
    fn zero_param_handler() {
        let mut handler: Box<dyn Handler<SpawnRequest, SpawnResponse>> =
            IntoHandler::<SpawnRequest, SpawnResponse, ()>::into_handler(
                spawn_no_params,
                "zero_param",
            );
        let mut world = World::new();
        let result = handler.run(SpawnRequest { unit_id: 1 }, &mut world);
        assert_eq!(result.unwrap(), SpawnResponse { ok: true });
    }

    fn spawn_with_res(_req: SpawnRequest, cfg: Res<'_, Config>) -> Result<SpawnResponse, String> {
        Ok(SpawnResponse {
            ok: cfg.multiplier > 0.0,
        })
    }

    #[test]
    fn one_param_res_handler() {
        let mut handler: Box<dyn Handler<SpawnRequest, SpawnResponse>> =
            IntoHandler::<SpawnRequest, SpawnResponse, (Res<'_, Config>,)>::into_handler(
                spawn_with_res,
                "one_param_res",
            );
        let mut world = World::new();
        world.insert_resource(Config { multiplier: 2.0 });
        let result = handler.run(SpawnRequest { unit_id: 1 }, &mut world);
        assert_eq!(result.unwrap(), SpawnResponse { ok: true });
    }

    fn spawn_with_res_mut(
        req: SpawnRequest,
        mut cfg: ResMut<'_, Config>,
    ) -> Result<SpawnResponse, String> {
        cfg.multiplier = req.unit_id as f32;
        Ok(SpawnResponse { ok: true })
    }

    #[test]
    fn one_param_res_mut_handler() {
        let mut handler: Box<dyn Handler<SpawnRequest, SpawnResponse>> =
            IntoHandler::<SpawnRequest, SpawnResponse, (ResMut<'_, Config>,)>::into_handler(
                spawn_with_res_mut,
                "one_param_res_mut",
            );
        let mut world = World::new();
        world.insert_resource(Config { multiplier: 0.0 });
        let result = handler.run(SpawnRequest { unit_id: 42 }, &mut world);
        assert_eq!(result.unwrap(), SpawnResponse { ok: true });
        assert!((world.resource::<Config>().multiplier - 42.0).abs() < f32::EPSILON);
    }

    fn spawn_with_query(
        req: SpawnRequest,
        counters: Query<'_, Counter>,
    ) -> Result<SpawnResponse, String> {
        Ok(SpawnResponse {
            ok: counters.len() as u64 == req.unit_id,
        })
    }

    #[test]
    fn one_param_query_handler() {
        let mut handler: Box<dyn Handler<SpawnRequest, SpawnResponse>> =
            IntoHandler::<SpawnRequest, SpawnResponse, (Query<'_, Counter>,)>::into_handler(
                spawn_with_query,
                "one_param_query",
            );
        let mut world = World::new();
        world.spawn((Counter(10),));
        world.spawn((Counter(20),));
        let result = handler.run(SpawnRequest { unit_id: 2 }, &mut world);
        assert_eq!(result.unwrap(), SpawnResponse { ok: true });
    }

    fn spawn_with_query_mut(
        req: SpawnRequest,
        mut counters: QueryMut<'_, Counter>,
    ) -> Result<SpawnResponse, String> {
        for (_, c) in counters.iter_mut() {
            c.0 += req.unit_id as u32;
        }
        Ok(SpawnResponse { ok: true })
    }

    #[test]
    fn one_param_query_mut_handler() {
        let mut handler: Box<dyn Handler<SpawnRequest, SpawnResponse>> =
            IntoHandler::<SpawnRequest, SpawnResponse, (QueryMut<'_, Counter>,)>::into_handler(
                spawn_with_query_mut,
                "one_param_query_mut",
            );
        let mut world = World::new();
        world.spawn((Counter(0),));
        world.spawn((Counter(5),));
        let result = handler.run(SpawnRequest { unit_id: 10 }, &mut world);
        assert_eq!(result.unwrap(), SpawnResponse { ok: true });
        let mut vals: Vec<u32> = world.query::<&Counter>().map(|(_, c)| c.0).collect();
        vals.sort();
        assert_eq!(vals, vec![10, 15]);
    }

    fn spawn_two_params(
        req: SpawnRequest,
        cfg: Res<'_, Config>,
        counters: Query<'_, Counter>,
    ) -> Result<SpawnResponse, String> {
        Ok(SpawnResponse {
            ok: cfg.multiplier > 0.0 && counters.len() as u64 == req.unit_id,
        })
    }

    #[test]
    fn two_param_handler() {
        let mut handler: Box<dyn Handler<SpawnRequest, SpawnResponse>> = IntoHandler::<
            SpawnRequest,
            SpawnResponse,
            (Res<'_, Config>, Query<'_, Counter>),
        >::into_handler(
            spawn_two_params,
            "two_param",
        );
        let mut world = World::new();
        world.insert_resource(Config { multiplier: 1.5 });
        world.spawn((Counter(0),));
        let result = handler.run(SpawnRequest { unit_id: 1 }, &mut world);
        assert_eq!(result.unwrap(), SpawnResponse { ok: true });
    }

    #[test]
    fn handler_reports_access() {
        let handler: Box<dyn Handler<SpawnRequest, SpawnResponse>> =
            IntoHandler::<SpawnRequest, SpawnResponse, (Res<'_, Config>,)>::into_handler(
                spawn_with_res,
                "access_check",
            );
        let access = handler.access();
        assert_eq!(access.len(), 1);
        assert!(matches!(access[0], Access::ResRead(_)));
    }

    #[test]
    fn run_handler_free_function() {
        let mut handler: Box<dyn Handler<SpawnRequest, SpawnResponse>> =
            IntoHandler::<SpawnRequest, SpawnResponse, ()>::into_handler(
                spawn_no_params,
                "free_fn",
            );
        let mut world = World::new();
        let result = run_handler(&mut *handler, SpawnRequest { unit_id: 99 }, &mut world);
        assert_eq!(result.unwrap(), SpawnResponse { ok: true });
    }

    #[test]
    fn run_json_handler_round_trip() {
        let mut handler: Box<dyn Handler<SpawnRequest, SpawnResponse>> =
            IntoHandler::<SpawnRequest, SpawnResponse, ()>::into_handler(
                spawn_no_params,
                "json_round_trip",
            );
        let mut world = World::new();
        let out = run_json_handler(&mut *handler, r#"{"unit_id":3}"#, &mut world).unwrap();
        assert_eq!(out, r#"{"ok":true}"#);
    }

    #[test]
    fn run_json_handler_function_infers_types() {
        let mut world = World::new();
        let out =
            run_json_handler_function(spawn_no_params, "json_fn", r#"{"unit_id":5}"#, &mut world)
                .unwrap();
        assert_eq!(out, r#"{"ok":true}"#);
    }

    fn json_round_trip_res_mut(
        req: SpawnRequest,
        mut cfg: ResMut<'_, Config>,
    ) -> Result<SpawnResponse, String> {
        cfg.multiplier = req.unit_id as f32;
        Ok(SpawnResponse { ok: true })
    }

    #[test]
    fn run_json_handler_res_mut_round_trip() {
        let mut handler: Box<dyn Handler<SpawnRequest, SpawnResponse>> =
            IntoHandler::<SpawnRequest, SpawnResponse, (ResMut<'_, Config>,)>::into_handler(
                json_round_trip_res_mut,
                "res_mut_json",
            );
        let mut world = World::new();
        world.insert_resource(Config { multiplier: 0.0 });
        let out = run_json_handler(&mut *handler, r#"{"unit_id":7}"#, &mut world).unwrap();
        assert_eq!(out, r#"{"ok":true}"#);
        assert!((world.resource::<Config>().multiplier - 7.0).abs() < f32::EPSILON);
    }

    #[test]
    fn run_json_handler_rejects_bad_json() {
        let mut handler: Box<dyn Handler<SpawnRequest, SpawnResponse>> =
            IntoHandler::<SpawnRequest, SpawnResponse, ()>::into_handler(
                spawn_no_params,
                "bad_json",
            );
        let mut world = World::new();
        let err = run_json_handler(&mut *handler, "not json", &mut world).unwrap_err();
        assert!(!err.is_empty());
    }

    // -------------------------------------------------------------------------
    // T5: Negative tests
    // -------------------------------------------------------------------------

    fn conflicting_handler(
        _req: SpawnRequest,
        _a: Res<'_, Config>,
        _b: ResMut<'_, Config>,
    ) -> Result<SpawnResponse, String> {
        Ok(SpawnResponse { ok: true })
    }

    #[test]
    #[should_panic(expected = "conflicting parameter access")]
    fn self_conflict_panics_on_registration() {
        let _ = IntoHandler::<
            SpawnRequest,
            SpawnResponse,
            (Res<'_, Config>, ResMut<'_, Config>),
        >::into_handler(conflicting_handler, "conflict");
    }

    fn failing_handler(_req: SpawnRequest) -> Result<SpawnResponse, String> {
        Err("some error".into())
    }

    #[test]
    fn handler_error_propagates() {
        let mut handler: Box<dyn Handler<SpawnRequest, SpawnResponse>> =
            IntoHandler::<SpawnRequest, SpawnResponse, ()>::into_handler(
                failing_handler,
                "error_handler",
            );
        let mut world = World::new();
        let result = handler.run(SpawnRequest { unit_id: 1 }, &mut world);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "some error");
    }

    // -- Domain error type tests --

    #[derive(Debug)]
    struct ApiError {
        code: u16,
        message: String,
    }

    impl std::fmt::Display for ApiError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "ApiError({}): {}", self.code, self.message)
        }
    }

    fn handler_with_domain_error(_req: SpawnRequest) -> Result<SpawnResponse, ApiError> {
        Err(ApiError {
            code: 404,
            message: "not found".into(),
        })
    }

    #[test]
    fn domain_error_type_converts_to_string() {
        let mut handler: Box<dyn Handler<SpawnRequest, SpawnResponse>> =
            IntoHandler::<SpawnRequest, SpawnResponse, ()>::into_handler(
                handler_with_domain_error,
                "domain_error",
            );
        let mut world = World::new();
        let result = handler.run(SpawnRequest { unit_id: 1 }, &mut world);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "ApiError(404): not found");
    }

    fn handler_with_domain_error_and_params(
        _req: SpawnRequest,
        _cfg: Res<'_, Config>,
    ) -> Result<SpawnResponse, ApiError> {
        Err(ApiError {
            code: 500,
            message: "internal".into(),
        })
    }

    #[test]
    fn domain_error_type_with_params() {
        let mut handler: Box<dyn Handler<SpawnRequest, SpawnResponse>> =
            IntoHandler::<SpawnRequest, SpawnResponse, (Res<'_, Config>,)>::into_handler(
                handler_with_domain_error_and_params,
                "domain_error_params",
            );
        let mut world = World::new();
        world.insert_resource(Config { multiplier: 1.0 });
        let result = handler.run(SpawnRequest { unit_id: 1 }, &mut world);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "ApiError(500): internal");
    }
}
