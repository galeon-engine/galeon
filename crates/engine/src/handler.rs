// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

//! Handler registration seam — the boundary between generated adapters and
//! game-owned domain logic.
//!
//! Galeon generates adapter glue. The game project implements handlers.
//! Both local and remote adapters target the same [`HandlerRegistry`].
//!
//! # Architecture
//!
//! ```text
//! Protocol definitions (game crate)
//!         │
//!         ▼
//! ┌─────────────────┐
//! │ HandlerRegistry │ ← game registers handlers here
//! └────────┬────────┘
//!          │
//!     ┌────┴────┐
//!     ▼         ▼
//!   Local    Remote
//!  Adapter   Adapter
//! ```
//!
//! # Design Rules
//!
//! - One command type → one handler entry
//! - One query type → one handler entry
//! - Game project owns all handlers; Galeon does not generate domain logic
//! - Local and remote adapters target the same boundary

use std::any::{Any, TypeId, type_name};
use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::protocol::{Command, ProtocolMeta, ProtocolQuery};

// =============================================================================
// Handler traits — game implements these
// =============================================================================

/// Handler for a command type. Game project implements this.
///
/// `C` is the command type (e.g., `DispatchShip`).
/// `R` is the response type (e.g., `()` or a result DTO).
pub trait CommandHandler<C: Command, R: Serialize>: Send + Sync {
    /// Execute the command and return a response.
    fn handle(&self, cmd: C) -> Result<R, String>;
}

/// Handler for a query type. Game project implements this.
///
/// `Q` is the query type (e.g., `GetFleetSnapshot`).
/// `R` is the response type (e.g., `FleetSnapshot` DTO).
pub trait QueryHandler<Q: ProtocolQuery, R: Serialize>: Send + Sync {
    /// Execute the query and return a response.
    fn handle(&self, query: Q) -> Result<R, String>;
}

// =============================================================================
// Type-erased handler wrappers (internal)
// =============================================================================

/// A type-erased command handler that works with JSON strings.
///
/// This is the boundary between typed game handlers and transport adapters.
trait ErasedCommandHandler: Send + Sync {
    /// Deserialize request JSON, call the typed handler, serialize response.
    fn handle_json(&self, request: &str) -> Result<String, String>;

    /// Call the typed handler with a boxed Any (for local adapter).
    fn handle_any(&self, cmd: Box<dyn Any>) -> Result<Box<dyn Any>, String>;
}

/// A type-erased query handler.
trait ErasedQueryHandler: Send + Sync {
    fn handle_json(&self, request: &str) -> Result<String, String>;
    fn handle_any(&self, query: Box<dyn Any>) -> Result<Box<dyn Any>, String>;
}

/// Wraps a typed CommandHandler into an erased one.
struct CommandHandlerWrapper<C, R, H> {
    handler: H,
    _phantom: std::marker::PhantomData<(C, R)>,
}

impl<C, R, H> ErasedCommandHandler for CommandHandlerWrapper<C, R, H>
where
    C: Command,
    R: Serialize + for<'de> Deserialize<'de> + Send + Sync + 'static,
    H: CommandHandler<C, R> + Send + Sync,
{
    fn handle_json(&self, request: &str) -> Result<String, String> {
        let cmd: C = serde_json::from_str(request).map_err(|e| e.to_string())?;
        let response = self.handler.handle(cmd)?;
        serde_json::to_string(&response).map_err(|e| e.to_string())
    }

    fn handle_any(&self, cmd: Box<dyn Any>) -> Result<Box<dyn Any>, String> {
        let cmd = *cmd.downcast::<C>().map_err(|_| "type mismatch")?;
        let response = self.handler.handle(cmd)?;
        Ok(Box::new(response))
    }
}

/// Wraps a typed QueryHandler into an erased one.
struct QueryHandlerWrapper<Q, R, H> {
    handler: H,
    _phantom: std::marker::PhantomData<(Q, R)>,
}

impl<Q, R, H> ErasedQueryHandler for QueryHandlerWrapper<Q, R, H>
where
    Q: ProtocolQuery,
    R: Serialize + for<'de> Deserialize<'de> + Send + Sync + 'static,
    H: QueryHandler<Q, R> + Send + Sync,
{
    fn handle_json(&self, request: &str) -> Result<String, String> {
        let query: Q = serde_json::from_str(request).map_err(|e| e.to_string())?;
        let response = self.handler.handle(query)?;
        serde_json::to_string(&response).map_err(|e| e.to_string())
    }

    fn handle_any(&self, query: Box<dyn Any>) -> Result<Box<dyn Any>, String> {
        let query = *query.downcast::<Q>().map_err(|_| "type mismatch")?;
        let response = self.handler.handle(query)?;
        Ok(Box::new(response))
    }
}

// =============================================================================
// HandlerRegistry — the registration seam
// =============================================================================

/// A stored handler entry shared between TypeId and name indices.
struct CommandEntry(std::sync::Arc<dyn ErasedCommandHandler>);
struct QueryEntry(std::sync::Arc<dyn ErasedQueryHandler>);

/// Registry of command and query handlers.
///
/// Game projects register handlers here. Both local and remote adapters
/// dispatch through this registry. Local dispatch uses `TypeId` (zero-cost).
/// Remote dispatch uses the stable protocol name from `ProtocolMeta::name()`.
pub struct HandlerRegistry {
    /// TypeId → handler index (local adapter path).
    commands_by_type: HashMap<TypeId, CommandEntry>,
    /// Protocol name → handler index (remote adapter path).
    commands_by_name: HashMap<String, CommandEntry>,
    /// TypeId → handler index (local adapter path).
    queries_by_type: HashMap<TypeId, QueryEntry>,
    /// Protocol name → handler index (remote adapter path).
    queries_by_name: HashMap<String, QueryEntry>,
}

impl HandlerRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            commands_by_type: HashMap::new(),
            commands_by_name: HashMap::new(),
            queries_by_type: HashMap::new(),
            queries_by_name: HashMap::new(),
        }
    }

    /// Register a command handler.
    ///
    /// Indexes by both `TypeId` (for local dispatch) and
    /// `ProtocolMeta::name()` (for remote dispatch via stable protocol name).
    ///
    /// Panics if a handler for this command type or protocol name is already
    /// registered. The name check catches collisions between different Rust
    /// types that share the same `ProtocolMeta::name()`.
    pub fn register_command<C, R, H>(&mut self, handler: H)
    where
        C: Command + ProtocolMeta,
        R: Serialize + for<'de> Deserialize<'de> + Send + Sync + 'static,
        H: CommandHandler<C, R> + 'static,
    {
        let type_id = TypeId::of::<C>();
        let protocol_name = C::name().to_string();
        assert!(
            !self.commands_by_type.contains_key(&type_id),
            "duplicate command handler for type {}",
            protocol_name
        );
        assert!(
            !self.commands_by_name.contains_key(&protocol_name),
            "duplicate command handler for protocol name {:?} (different type, same name)",
            protocol_name
        );
        let shared: std::sync::Arc<dyn ErasedCommandHandler> =
            std::sync::Arc::new(CommandHandlerWrapper {
                handler,
                _phantom: std::marker::PhantomData::<(C, R)>,
            });
        self.commands_by_type
            .insert(type_id, CommandEntry(shared.clone()));
        self.commands_by_name
            .insert(protocol_name, CommandEntry(shared));
    }

    /// Register a query handler.
    ///
    /// Indexes by both `TypeId` (for local dispatch) and
    /// `ProtocolMeta::name()` (for remote dispatch via stable protocol name).
    ///
    /// Panics if a handler for this query type or protocol name is already
    /// registered. The name check catches collisions between different Rust
    /// types that share the same `ProtocolMeta::name()`.
    pub fn register_query<Q, R, H>(&mut self, handler: H)
    where
        Q: ProtocolQuery + ProtocolMeta,
        R: Serialize + for<'de> Deserialize<'de> + Send + Sync + 'static,
        H: QueryHandler<Q, R> + 'static,
    {
        let type_id = TypeId::of::<Q>();
        let protocol_name = Q::name().to_string();
        assert!(
            !self.queries_by_type.contains_key(&type_id),
            "duplicate query handler for type {}",
            protocol_name
        );
        assert!(
            !self.queries_by_name.contains_key(&protocol_name),
            "duplicate query handler for protocol name {:?} (different type, same name)",
            protocol_name
        );
        let shared: std::sync::Arc<dyn ErasedQueryHandler> =
            std::sync::Arc::new(QueryHandlerWrapper {
                handler,
                _phantom: std::marker::PhantomData::<(Q, R)>,
            });
        self.queries_by_type
            .insert(type_id, QueryEntry(shared.clone()));
        self.queries_by_name
            .insert(protocol_name, QueryEntry(shared));
    }

    // -------------------------------------------------------------------------
    // Local adapter interface (in-process, typed dispatch)
    // -------------------------------------------------------------------------

    /// Dispatch a command in-process (local adapter path).
    pub fn dispatch_command<C: Command + 'static, R: 'static>(&self, cmd: C) -> Result<R, String> {
        let entry = self
            .commands_by_type
            .get(&TypeId::of::<C>())
            .ok_or_else(|| format!("no handler for command {}", type_name::<C>()))?;

        let result = entry.0.handle_any(Box::new(cmd))?;
        let boxed = result
            .downcast::<R>()
            .map_err(|_| "response type mismatch".to_string())?;
        Ok(*boxed)
    }

    /// Dispatch a query in-process (local adapter path).
    pub fn dispatch_query<Q: ProtocolQuery + 'static, R: 'static>(
        &self,
        query: Q,
    ) -> Result<R, String> {
        let entry = self
            .queries_by_type
            .get(&TypeId::of::<Q>())
            .ok_or_else(|| format!("no handler for query {}", type_name::<Q>()))?;

        let result = entry.0.handle_any(Box::new(query))?;
        let boxed = result
            .downcast::<R>()
            .map_err(|_| "response type mismatch".to_string())?;
        Ok(*boxed)
    }

    // -------------------------------------------------------------------------
    // Remote adapter interface (JSON boundary, keyed by stable protocol name)
    // -------------------------------------------------------------------------

    /// Dispatch a command via JSON using the stable protocol name.
    ///
    /// `protocol_name` is the value from `ProtocolMeta::name()` (e.g.,
    /// `"DispatchShip"`) — the same name that appears in the manifest and
    /// generated descriptors. This is the boundary-safe dispatch path.
    pub fn dispatch_command_json(
        &self,
        protocol_name: &str,
        request_json: &str,
    ) -> Result<String, String> {
        let entry = self
            .commands_by_name
            .get(protocol_name)
            .ok_or_else(|| format!("unknown command: {}", protocol_name))?;
        entry.0.handle_json(request_json)
    }

    /// Dispatch a query via JSON using the stable protocol name.
    pub fn dispatch_query_json(
        &self,
        protocol_name: &str,
        request_json: &str,
    ) -> Result<String, String> {
        let entry = self
            .queries_by_name
            .get(protocol_name)
            .ok_or_else(|| format!("unknown query: {}", protocol_name))?;
        entry.0.handle_json(request_json)
    }

    /// Returns the number of registered command handlers.
    pub fn command_count(&self) -> usize {
        self.commands_by_type.len()
    }

    /// Returns the number of registered query handlers.
    pub fn query_count(&self) -> usize {
        self.queries_by_type.len()
    }
}

impl Default for HandlerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -- Sample protocol items --

    use crate::protocol::ProtocolKind;

    #[derive(Debug, Serialize, Deserialize)]
    struct DispatchShip {
        ship_id: u64,
        contract_id: u64,
    }
    impl Command for DispatchShip {}
    impl ProtocolMeta for DispatchShip {
        fn name() -> &'static str {
            "DispatchShip"
        }
        fn kind() -> ProtocolKind {
            ProtocolKind::Command
        }
    }

    #[derive(Debug, Serialize, Deserialize)]
    struct GetFleetSnapshot;
    impl ProtocolQuery for GetFleetSnapshot {}
    impl ProtocolMeta for GetFleetSnapshot {
        fn name() -> &'static str {
            "GetFleetSnapshot"
        }
        fn kind() -> ProtocolKind {
            ProtocolKind::Query
        }
    }

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    struct FleetSnapshot {
        ships_in_transit: u32,
        ships_docked: u32,
    }

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    struct DispatchResult {
        ok: bool,
    }

    // -- Sample handlers --

    struct ShipDispatcher;

    impl CommandHandler<DispatchShip, DispatchResult> for ShipDispatcher {
        fn handle(&self, _cmd: DispatchShip) -> Result<DispatchResult, String> {
            Ok(DispatchResult { ok: true })
        }
    }

    struct FleetQuerier;

    impl QueryHandler<GetFleetSnapshot, FleetSnapshot> for FleetQuerier {
        fn handle(&self, _query: GetFleetSnapshot) -> Result<FleetSnapshot, String> {
            Ok(FleetSnapshot {
                ships_in_transit: 2,
                ships_docked: 5,
            })
        }
    }

    // -- Registry tests --

    #[test]
    fn register_and_dispatch_command_local() {
        let mut registry = HandlerRegistry::new();
        registry.register_command::<DispatchShip, DispatchResult, _>(ShipDispatcher);

        let result: DispatchResult = registry
            .dispatch_command(DispatchShip {
                ship_id: 1,
                contract_id: 42,
            })
            .unwrap();

        assert!(result.ok);
    }

    #[test]
    fn register_and_dispatch_query_local() {
        let mut registry = HandlerRegistry::new();
        registry.register_query::<GetFleetSnapshot, FleetSnapshot, _>(FleetQuerier);

        let snapshot: FleetSnapshot = registry.dispatch_query(GetFleetSnapshot).unwrap();

        assert_eq!(snapshot.ships_in_transit, 2);
        assert_eq!(snapshot.ships_docked, 5);
    }

    #[test]
    fn dispatch_command_json_by_protocol_name() {
        let mut registry = HandlerRegistry::new();
        registry.register_command::<DispatchShip, DispatchResult, _>(ShipDispatcher);

        // Use stable protocol name — the same name in manifest/descriptors.
        let response = registry
            .dispatch_command_json("DispatchShip", r#"{"ship_id":1,"contract_id":42}"#)
            .unwrap();

        assert!(response.contains("true"));
    }

    #[test]
    fn dispatch_query_json_by_protocol_name() {
        let mut registry = HandlerRegistry::new();
        registry.register_query::<GetFleetSnapshot, FleetSnapshot, _>(FleetQuerier);

        let response = registry
            .dispatch_query_json("GetFleetSnapshot", "null")
            .unwrap();

        let snapshot: FleetSnapshot = serde_json::from_str(&response).unwrap();
        assert_eq!(snapshot.ships_docked, 5);
    }

    #[test]
    fn same_registry_serves_both_adapters() {
        let mut registry = HandlerRegistry::new();
        registry.register_command::<DispatchShip, DispatchResult, _>(ShipDispatcher);
        registry.register_query::<GetFleetSnapshot, FleetSnapshot, _>(FleetQuerier);

        // Local adapter (typed, in-process)
        let local_result: DispatchResult = registry
            .dispatch_command(DispatchShip {
                ship_id: 1,
                contract_id: 42,
            })
            .unwrap();
        assert!(local_result.ok);

        let local_snapshot: FleetSnapshot = registry.dispatch_query(GetFleetSnapshot).unwrap();
        assert_eq!(local_snapshot.ships_docked, 5);

        // Remote adapter (JSON, keyed by stable protocol name)
        let json_result = registry
            .dispatch_command_json("DispatchShip", r#"{"ship_id":2,"contract_id":99}"#)
            .unwrap();
        assert!(json_result.contains("true"));

        let json_snapshot = registry
            .dispatch_query_json("GetFleetSnapshot", "null")
            .unwrap();
        let remote_snapshot: FleetSnapshot = serde_json::from_str(&json_snapshot).unwrap();
        assert_eq!(remote_snapshot.ships_docked, 5);
    }

    /// Drives remote dispatch from descriptor output — proves the
    /// execution-portability claim: descriptor names resolve to handlers.
    #[test]
    fn descriptor_driven_remote_dispatch() {
        use crate::codegen::generate_descriptors;
        use crate::manifest::{ManifestEntry, ManifestField, ProtocolManifest};

        // Build a manifest matching our test protocol items.
        let manifest = ProtocolManifest {
            manifest_version: "1".into(),
            protocol_version: "test@0.1".into(),
            commands: vec![ManifestEntry {
                name: "DispatchShip".into(),
                kind: ProtocolKind::Command,
                fields: vec![
                    ManifestField {
                        name: "ship_id".into(),
                        ty: "u64".into(),
                    },
                    ManifestField {
                        name: "contract_id".into(),
                        ty: "u64".into(),
                    },
                ],
                doc: "".into(),
            }],
            queries: vec![ManifestEntry {
                name: "GetFleetSnapshot".into(),
                kind: ProtocolKind::Query,
                fields: vec![],
                doc: "".into(),
            }],
            events: vec![],
            dtos: vec![],
        };

        // Generate descriptors (simulating what codegen produces).
        let desc_set = generate_descriptors(&manifest);

        // Register handlers.
        let mut registry = HandlerRegistry::new();
        registry.register_command::<DispatchShip, DispatchResult, _>(ShipDispatcher);
        registry.register_query::<GetFleetSnapshot, FleetSnapshot, _>(FleetQuerier);

        // Dispatch using descriptor names — no TypeId, no Rust-only knowledge.
        for desc in &desc_set.descriptors {
            match desc.kind {
                ProtocolKind::Command => {
                    let response = registry
                        .dispatch_command_json(&desc.name, r#"{"ship_id":1,"contract_id":42}"#)
                        .unwrap();
                    assert!(response.contains("true"));
                }
                ProtocolKind::Query => {
                    let response = registry.dispatch_query_json(&desc.name, "null").unwrap();
                    let snapshot: FleetSnapshot = serde_json::from_str(&response).unwrap();
                    assert_eq!(snapshot.ships_docked, 5);
                }
                _ => {}
            }
        }
    }

    #[test]
    fn missing_handler_returns_error() {
        let registry = HandlerRegistry::new();
        let result = registry.dispatch_command::<DispatchShip, DispatchResult>(DispatchShip {
            ship_id: 1,
            contract_id: 1,
        });
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("no handler"));
    }

    #[test]
    #[should_panic(expected = "duplicate command handler")]
    fn duplicate_handler_panics() {
        let mut registry = HandlerRegistry::new();
        registry.register_command::<DispatchShip, DispatchResult, _>(ShipDispatcher);
        registry.register_command::<DispatchShip, DispatchResult, _>(ShipDispatcher);
    }

    #[test]
    fn registry_counts() {
        let mut registry = HandlerRegistry::new();
        assert_eq!(registry.command_count(), 0);
        assert_eq!(registry.query_count(), 0);

        registry.register_command::<DispatchShip, DispatchResult, _>(ShipDispatcher);
        registry.register_query::<GetFleetSnapshot, FleetSnapshot, _>(FleetQuerier);

        assert_eq!(registry.command_count(), 1);
        assert_eq!(registry.query_count(), 1);
    }

    /// Two different Rust types with the same ProtocolMeta::name() must
    /// panic on registration — prevents silent handler replacement.
    #[test]
    #[should_panic(expected = "duplicate command handler for protocol name")]
    fn name_collision_panics() {
        // A second command type that shares the same protocol name.
        #[derive(Debug, Serialize, Deserialize)]
        struct DispatchShipV2 {
            ship_id: u64,
        }
        impl Command for DispatchShipV2 {}
        impl ProtocolMeta for DispatchShipV2 {
            fn name() -> &'static str {
                "DispatchShip" // same name as the other type
            }
            fn kind() -> ProtocolKind {
                ProtocolKind::Command
            }
        }

        struct V2Handler;
        impl CommandHandler<DispatchShipV2, DispatchResult> for V2Handler {
            fn handle(&self, _cmd: DispatchShipV2) -> Result<DispatchResult, String> {
                Ok(DispatchResult { ok: false })
            }
        }

        let mut registry = HandlerRegistry::new();
        registry.register_command::<DispatchShip, DispatchResult, _>(ShipDispatcher);
        // This must panic — same protocol name, different type.
        registry.register_command::<DispatchShipV2, DispatchResult, _>(V2Handler);
    }
}
