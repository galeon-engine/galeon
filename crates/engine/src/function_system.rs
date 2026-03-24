// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

use std::marker::PhantomData;

use crate::system_param::{Access, SystemParam};
use crate::world::World;

// =============================================================================
// System trait — trait-object interface for all system types
// =============================================================================

/// Trait-object interface for all system types.
pub trait System {
    /// Human-readable system name (for diagnostics and conflict messages).
    fn name(&self) -> &'static str;

    /// Run the system against the world.
    fn run(&mut self, world: &mut World);

    /// Declare what world data this system accesses.
    fn access(&self) -> Vec<Access>;
}

// =============================================================================
// IntoSystem — converts a compatible function into a boxed System
// =============================================================================

/// Converts a compatible function into a boxed [`System`].
///
/// Implemented for:
/// - `fn(&mut World)` (legacy systems, `Params = ()`)
/// - `fn(P0, P1, ...)` where each `P` is a [`SystemParam`]
pub trait IntoSystem<Params> {
    fn into_system(self, name: &'static str) -> Box<dyn System>;
}

// =============================================================================
// Legacy system — fn(&mut World)
// =============================================================================

type LegacySystemFn = fn(&mut World);

struct LegacySystem {
    name: &'static str,
    func: LegacySystemFn,
}

impl System for LegacySystem {
    fn name(&self) -> &'static str {
        self.name
    }

    fn run(&mut self, world: &mut World) {
        (self.func)(world);
    }

    fn access(&self) -> Vec<Access> {
        // Legacy systems have opaque access — they take &mut World.
        Vec::new()
    }
}

impl IntoSystem<()> for LegacySystemFn {
    fn into_system(self, name: &'static str) -> Box<dyn System> {
        Box::new(LegacySystem { name, func: self })
    }
}

// =============================================================================
// Parameterized system — fn(P0, P1, ...) where each P: SystemParam
// =============================================================================

/// Marker trait bridging an `FnMut(P::Item<'_>, ...)` to `System::run`.
pub trait SystemParamFunction<Params>: 'static {
    fn run(&mut self, world: &mut World);
    fn param_access() -> Vec<Access>;
}

struct ParamSystem<F, Params> {
    name: &'static str,
    func: F,
    _marker: PhantomData<fn() -> Params>,
}

impl<F, Params> System for ParamSystem<F, Params>
where
    F: SystemParamFunction<Params>,
{
    fn name(&self) -> &'static str {
        self.name
    }

    fn run(&mut self, world: &mut World) {
        self.func.run(world);
    }

    fn access(&self) -> Vec<Access> {
        F::param_access()
    }
}

/// Panics if any two accesses within the same system conflict.
fn validate_no_self_conflicts(access: &[Access], system_name: &'static str) {
    for (i, a) in access.iter().enumerate() {
        for b in &access[i + 1..] {
            if a.conflicts_with(b) {
                panic!(
                    "system '{}' has conflicting parameter access: {:?} vs {:?}",
                    system_name, a, b,
                );
            }
        }
    }
}

// =============================================================================
// Arity macros — 1..8 parameter systems
// =============================================================================

macro_rules! impl_system_param_function {
    ($($P:ident),+) => {
        #[allow(non_snake_case)]
        impl<Func, $($P: SystemParam + 'static),+> SystemParamFunction<($($P,)+)> for Func
        where
            Func: FnMut($($P::Item<'_>),+) + 'static,
        {
            fn run(&mut self, world: &mut World) {
                let world_ptr = world as *mut World;
                // SAFETY: access conflicts checked at registration time via
                // validate_no_self_conflicts. Each resource/component lives in
                // its own heap allocation, so distinct TypeId accesses cannot
                // alias even through the same *mut World.
                unsafe {
                    self($($P::fetch(world_ptr),)+);
                }
            }

            fn param_access() -> Vec<Access> {
                let mut acc = Vec::new();
                $(acc.extend($P::access());)+
                acc
            }
        }

        impl<Func, $($P: SystemParam + 'static),+> IntoSystem<($($P,)+)> for Func
        where
            Func: SystemParamFunction<($($P,)+)>,
        {
            fn into_system(self, name: &'static str) -> Box<dyn System> {
                let access = <Func as SystemParamFunction<($($P,)+)>>::param_access();
                validate_no_self_conflicts(&access, name);
                Box::new(ParamSystem {
                    name,
                    func: self,
                    _marker: PhantomData,
                })
            }
        }
    };
}

impl_system_param_function!(P0);
impl_system_param_function!(P0, P1);
impl_system_param_function!(P0, P1, P2);
impl_system_param_function!(P0, P1, P2, P3);
impl_system_param_function!(P0, P1, P2, P3, P4);
impl_system_param_function!(P0, P1, P2, P3, P4, P5);
impl_system_param_function!(P0, P1, P2, P3, P4, P5, P6);
impl_system_param_function!(P0, P1, P2, P3, P4, P5, P6, P7);

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::component::Component;
    use crate::system_param::{Query, QueryMut, Res, ResMut};

    #[derive(Debug)]
    struct Counter(u32);
    impl Component for Counter {}

    struct Speed(f32);

    fn legacy_increment(world: &mut World) {
        for (_, c) in world.query_mut::<&mut Counter>() {
            c.0 += 1;
        }
    }

    #[test]
    fn legacy_into_system() {
        let mut sys: Box<dyn System> =
            IntoSystem::<()>::into_system(legacy_increment as LegacySystemFn, "legacy");
        let mut world = World::new();
        world.spawn((Counter(0),));
        sys.run(&mut world);
        let vals: Vec<u32> = world.query::<&Counter>().map(|(_, c)| c.0).collect();
        assert_eq!(vals, vec![1]);
    }

    fn count_entities(query: Query<'_, Counter>) {
        let _ = query.len();
    }

    #[test]
    fn one_param_into_system() {
        let mut sys: Box<dyn System> =
            IntoSystem::<(Query<'_, Counter>,)>::into_system(count_entities, "count");
        let mut world = World::new();
        world.spawn((Counter(0),));
        sys.run(&mut world);
    }

    fn read_speed_count_entities(speed: Res<'_, Speed>, query: Query<'_, Counter>) {
        let _ = *speed;
        let _ = query.len();
    }

    #[test]
    fn two_param_into_system() {
        let mut sys: Box<dyn System> =
            IntoSystem::<(Res<'_, Speed>, Query<'_, Counter>)>::into_system(
                read_speed_count_entities,
                "two_param",
            );
        let mut world = World::new();
        world.insert_resource(Speed(1.5));
        world.spawn((Counter(0),));
        sys.run(&mut world);
    }

    fn increment_speed(mut speed: ResMut<'_, Speed>) {
        speed.0 += 1.0;
    }

    #[test]
    fn res_mut_system_mutates() {
        let mut sys: Box<dyn System> =
            IntoSystem::<(ResMut<'_, Speed>,)>::into_system(increment_speed, "inc_speed");
        let mut world = World::new();
        world.insert_resource(Speed(0.0));
        sys.run(&mut world);
        assert!((world.resource::<Speed>().0 - 1.0).abs() < f32::EPSILON);
    }

    fn increment_counters(mut counters: QueryMut<'_, Counter>) {
        for (_, c) in counters.iter_mut() {
            c.0 += 1;
        }
    }

    #[test]
    fn query_mut_system_mutates() {
        let mut sys: Box<dyn System> =
            IntoSystem::<(QueryMut<'_, Counter>,)>::into_system(increment_counters, "inc_counters");
        let mut world = World::new();
        world.spawn((Counter(0),));
        world.spawn((Counter(10),));
        sys.run(&mut world);
        let mut vals: Vec<u32> = world.query::<&Counter>().map(|(_, c)| c.0).collect();
        vals.sort();
        assert_eq!(vals, vec![1, 11]);
    }

    fn conflicting_system(_a: Res<'_, Speed>, _b: ResMut<'_, Speed>) {}

    #[test]
    #[should_panic(expected = "conflicting parameter access")]
    fn self_conflict_panics_on_registration() {
        let _ = IntoSystem::<(Res<'_, Speed>, ResMut<'_, Speed>)>::into_system(
            conflicting_system,
            "conflict",
        );
    }

    #[test]
    fn system_reports_access() {
        let sys: Box<dyn System> =
            IntoSystem::<(ResMut<'_, Speed>,)>::into_system(increment_speed, "inc_speed");
        let access = sys.access();
        assert_eq!(access.len(), 1);
    }
}
