// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

use std::any::TypeId;
use std::ops::{Deref, DerefMut};

use crate::component::Component;
use crate::entity::Entity;
use crate::query::{QueryIter, QueryIterMut};
use crate::world::UnsafeWorldCell;

// =============================================================================
// Access — describes what a system parameter touches
// =============================================================================

/// Describes what a system parameter accesses — used for conflict detection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Access {
    /// Shared read of a resource.
    ResRead(TypeId),
    /// Exclusive write of a resource.
    ResWrite(TypeId),
    /// Shared read of a component type.
    CompRead(TypeId),
    /// Exclusive write of a component type.
    CompWrite(TypeId),
}

impl Access {
    /// Returns `true` if `self` and `other` cannot safely coexist in the same
    /// system execution.
    ///
    /// Conflict rules:
    /// - Read  + Write (same TypeId, same namespace) → conflict
    /// - Write + Write (same TypeId, same namespace) → conflict
    /// - Read  + Read  → no conflict
    /// - Cross-namespace (Res vs Comp) → no conflict
    pub fn conflicts_with(&self, other: &Access) -> bool {
        match (self, other) {
            (Access::ResRead(a), Access::ResWrite(b))
            | (Access::ResWrite(a), Access::ResRead(b))
            | (Access::ResWrite(a), Access::ResWrite(b)) => a == b,

            (Access::CompRead(a), Access::CompWrite(b))
            | (Access::CompWrite(a), Access::CompRead(b))
            | (Access::CompWrite(a), Access::CompWrite(b)) => a == b,

            _ => false,
        }
    }
}

/// Returns `true` if any access in `a` conflicts with any in `b`.
pub fn has_conflicts(a: &[Access], b: &[Access]) -> bool {
    a.iter().any(|x| b.iter().any(|y| x.conflicts_with(y)))
}

// =============================================================================
// SystemParam trait
// =============================================================================

/// A type that can be extracted from a `World` as a system parameter.
///
/// # Safety
///
/// Implementations must correctly report all data accessed via `access()`.
/// `fetch()` may only touch the data declared in `access()`. The caller
/// guarantees that no other parameter has aliasing mutable access to the same
/// data — enforced at system registration time by conflict detection.
///
/// Each resource/component lives in its own heap allocation (`Box` inside
/// `HashMap`), so accesses to different `TypeId`s do not alias even through
/// the same `*mut World`.
pub unsafe trait SystemParam {
    /// The concrete type produced for a given world lifetime.
    type Item<'w>;

    /// Declare what world data this parameter accesses.
    fn access() -> Vec<Access>;

    /// Extract the parameter from the world.
    ///
    /// # Safety
    ///
    /// Caller must guarantee no aliasing mutable access to the data declared
    /// in `access()`. The cell provides field-level access via `addr_of!`
    /// to avoid creating intermediate `&World` or `&mut World` references,
    /// preventing Stacked Borrows aliasing UB when multiple params are
    /// fetched in sequence.
    unsafe fn fetch<'w>(world: UnsafeWorldCell) -> Self::Item<'w>;
}

// =============================================================================
// Res<T> — shared resource access
// =============================================================================

/// Shared read access to a world resource.
pub struct Res<'w, T: 'static> {
    value: &'w T,
}

impl<T: 'static> Deref for Res<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.value
    }
}

// SAFETY: access() correctly reports ResRead. fetch() only reads the resource.
unsafe impl<T: 'static> SystemParam for Res<'_, T> {
    type Item<'w> = Res<'w, T>;

    fn access() -> Vec<Access> {
        vec![Access::ResRead(TypeId::of::<T>())]
    }

    unsafe fn fetch<'w>(world: UnsafeWorldCell) -> Res<'w, T> {
        Res {
            value: unsafe { world.get_resource::<T>() },
        }
    }
}

// =============================================================================
// ResMut<T> — exclusive resource access
// =============================================================================

/// Exclusive write access to a world resource.
pub struct ResMut<'w, T: 'static> {
    value: &'w mut T,
}

impl<T: 'static> Deref for ResMut<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.value
    }
}

impl<T: 'static> DerefMut for ResMut<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        self.value
    }
}

// SAFETY: access() correctly reports ResWrite. fetch() only mutates this resource.
unsafe impl<T: 'static> SystemParam for ResMut<'_, T> {
    type Item<'w> = ResMut<'w, T>;

    fn access() -> Vec<Access> {
        vec![Access::ResWrite(TypeId::of::<T>())]
    }

    unsafe fn fetch<'w>(world: UnsafeWorldCell) -> ResMut<'w, T> {
        ResMut {
            value: unsafe { world.get_resource_mut::<T>() },
        }
    }
}

// =============================================================================
// Query<T> — shared component query
// =============================================================================

/// Shared read access to all entities with component `T`.
pub struct Query<'w, T: Component> {
    results: Vec<(Entity, &'w T)>,
}

impl<'w, T: Component> Query<'w, T> {
    /// Iterate over matching `(Entity, &T)` pairs.
    pub fn iter(&self) -> impl Iterator<Item = (Entity, &T)> {
        self.results.iter().map(|&(e, v)| (e, v))
    }

    /// Returns `true` if no entities matched.
    pub fn is_empty(&self) -> bool {
        self.results.is_empty()
    }

    /// Returns the number of matching entities.
    pub fn len(&self) -> usize {
        self.results.len()
    }
}

// SAFETY: access() correctly reports CompRead. fetch() collects an immutable
// query — the archetype iterator borrows world.archetypes immutably via
// UnsafeWorldCell::archetypes() (no intermediate &World).
unsafe impl<T: Component> SystemParam for Query<'_, T> {
    type Item<'w> = Query<'w, T>;

    fn access() -> Vec<Access> {
        vec![Access::CompRead(TypeId::of::<T>())]
    }

    unsafe fn fetch<'w>(world: UnsafeWorldCell) -> Query<'w, T> {
        Query {
            results: unsafe { QueryIter::<'w, &T>::new(world.archetypes()).collect() },
        }
    }
}

// =============================================================================
// QueryMut<T> — exclusive component query
// =============================================================================

/// Exclusive write access to all entities with component `T`.
pub struct QueryMut<'w, T: Component> {
    results: Vec<(Entity, &'w mut T)>,
}

impl<'w, T: Component> QueryMut<'w, T> {
    /// Iterate mutably over matching `(Entity, &mut T)` pairs.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (Entity, &mut T)> + '_ {
        self.results.iter_mut().map(|(e, v)| (*e, &mut **v))
    }

    /// Returns `true` if no entities matched.
    pub fn is_empty(&self) -> bool {
        self.results.is_empty()
    }

    /// Returns the number of matching entities.
    pub fn len(&self) -> usize {
        self.results.len()
    }
}

// SAFETY: access() correctly reports CompWrite. fetch() collects a mutable
// query — the archetype iterator yields `&'w mut T` references into distinct
// column heap allocations per archetype. Uses archetypes_mut_ptr() to get a
// raw pointer, and QueryIterMut::new_from_ptr() to avoid creating
// `&mut ArchetypeStore` — preventing overlap with concurrent `&ArchetypeStore`
// from Query params.
unsafe impl<T: Component> SystemParam for QueryMut<'_, T> {
    type Item<'w> = QueryMut<'w, T>;

    fn access() -> Vec<Access> {
        vec![Access::CompWrite(TypeId::of::<T>())]
    }

    unsafe fn fetch<'w>(world: UnsafeWorldCell) -> QueryMut<'w, T> {
        QueryMut {
            results: unsafe {
                QueryIterMut::<'w, &mut T>::new_from_ptr(world.archetypes_mut_ptr()).collect()
            },
        }
    }
}

// =============================================================================
// Unit tuple — no parameters
// =============================================================================

// SAFETY: No access, no fetch.
unsafe impl SystemParam for () {
    type Item<'w> = ();

    fn access() -> Vec<Access> {
        Vec::new()
    }

    unsafe fn fetch<'w>(_world: UnsafeWorldCell) -> Self::Item<'w> {}
}

// =============================================================================
// Tuple expansion — 1..8 arity
// =============================================================================

macro_rules! impl_system_param_tuple {
    ($($P:ident),+) => {
        // SAFETY: access() is the union of all inner accesses.
        unsafe impl<$($P: SystemParam),+> SystemParam for ($($P,)+) {
            type Item<'w> = ($($P::Item<'w>,)+);

            fn access() -> Vec<Access> {
                let mut acc = Vec::new();
                $(acc.extend($P::access());)+
                acc
            }

            unsafe fn fetch<'w>(world: UnsafeWorldCell) -> Self::Item<'w> {
                ($(unsafe { $P::fetch(world) },)+)
            }
        }
    };
}

impl_system_param_tuple!(P0);
impl_system_param_tuple!(P0, P1);
impl_system_param_tuple!(P0, P1, P2);
impl_system_param_tuple!(P0, P1, P2, P3);
impl_system_param_tuple!(P0, P1, P2, P3, P4);
impl_system_param_tuple!(P0, P1, P2, P3, P4, P5);
impl_system_param_tuple!(P0, P1, P2, P3, P4, P5, P6);
impl_system_param_tuple!(P0, P1, P2, P3, P4, P5, P6, P7);

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::component::Component;
    use crate::world::World;

    fn type_id<T: 'static>() -> TypeId {
        TypeId::of::<T>()
    }

    #[derive(Debug, PartialEq)]
    struct Pos {
        x: f32,
    }
    impl Component for Pos {}

    // -- Access conflict tests --

    #[test]
    fn read_read_same_type_no_conflict() {
        let a = Access::ResRead(type_id::<u32>());
        let b = Access::ResRead(type_id::<u32>());
        assert!(!a.conflicts_with(&b));
    }

    #[test]
    fn read_write_same_type_conflicts() {
        let a = Access::ResRead(type_id::<u32>());
        let b = Access::ResWrite(type_id::<u32>());
        assert!(a.conflicts_with(&b));
        assert!(b.conflicts_with(&a));
    }

    #[test]
    fn write_write_same_type_conflicts() {
        let a = Access::ResWrite(type_id::<u32>());
        let b = Access::ResWrite(type_id::<u32>());
        assert!(a.conflicts_with(&b));
    }

    #[test]
    fn read_write_different_type_no_conflict() {
        let a = Access::ResRead(type_id::<u32>());
        let b = Access::ResWrite(type_id::<f32>());
        assert!(!a.conflicts_with(&b));
    }

    #[test]
    fn comp_read_write_conflicts() {
        let a = Access::CompRead(type_id::<u64>());
        let b = Access::CompWrite(type_id::<u64>());
        assert!(a.conflicts_with(&b));
        assert!(b.conflicts_with(&a));
    }

    #[test]
    fn res_and_comp_same_type_no_conflict() {
        let a = Access::ResWrite(type_id::<u32>());
        let b = Access::CompWrite(type_id::<u32>());
        assert!(!a.conflicts_with(&b));
    }

    #[test]
    fn has_conflicts_finds_conflict_in_sets() {
        let set_a = vec![
            Access::ResRead(type_id::<u32>()),
            Access::CompRead(type_id::<f32>()),
        ];
        let set_b = vec![
            Access::ResWrite(type_id::<u32>()),
            Access::CompRead(type_id::<f32>()),
        ];
        assert!(has_conflicts(&set_a, &set_b));
    }

    #[test]
    fn has_conflicts_empty_sets_no_conflict() {
        assert!(!has_conflicts(&[], &[]));
        assert!(!has_conflicts(&[Access::ResRead(type_id::<u32>())], &[]));
        assert!(!has_conflicts(&[], &[Access::ResWrite(type_id::<u32>())]));
    }

    // -- Res / ResMut fetch tests --

    #[test]
    fn res_fetches_resource() {
        let mut world = World::new();
        world.insert_resource(42_i32);
        let cell = unsafe { UnsafeWorldCell::new(&mut world as *mut World) };
        unsafe {
            let res: Res<'_, i32> = <Res<'_, i32> as SystemParam>::fetch(cell);
            assert_eq!(*res, 42);
        }
    }

    #[test]
    fn res_access_is_read() {
        let access = <Res<'_, i32> as SystemParam>::access();
        assert_eq!(access, vec![Access::ResRead(TypeId::of::<i32>())]);
    }

    #[test]
    fn res_mut_fetches_and_mutates() {
        let mut world = World::new();
        world.insert_resource(10_u32);
        let cell = unsafe { UnsafeWorldCell::new(&mut world as *mut World) };
        unsafe {
            let mut res: ResMut<'_, u32> = <ResMut<'_, u32> as SystemParam>::fetch(cell);
            *res = 20;
        }
        assert_eq!(*world.resource::<u32>(), 20);
    }

    #[test]
    fn res_mut_access_is_write() {
        let access = <ResMut<'_, u32> as SystemParam>::access();
        assert_eq!(access, vec![Access::ResWrite(TypeId::of::<u32>())]);
    }

    #[test]
    fn res_and_res_mut_different_types_no_conflict() {
        let a = <Res<'_, i32> as SystemParam>::access();
        let b = <ResMut<'_, u32> as SystemParam>::access();
        assert!(!has_conflicts(&a, &b));
    }

    #[test]
    fn res_and_res_mut_same_type_conflicts() {
        let a = <Res<'_, i32> as SystemParam>::access();
        let b = <ResMut<'_, i32> as SystemParam>::access();
        assert!(has_conflicts(&a, &b));
    }

    // -- Query / QueryMut fetch tests --

    #[test]
    fn query_fetches_matching_entities() {
        let mut world = World::new();
        world.spawn((Pos { x: 1.0 },));
        world.spawn((Pos { x: 2.0 },));
        let cell = unsafe { UnsafeWorldCell::new(&mut world as *mut World) };
        unsafe {
            let q: Query<'_, Pos> = <Query<'_, Pos> as SystemParam>::fetch(cell);
            assert_eq!(q.len(), 2);
        }
    }

    #[test]
    fn query_access_is_comp_read() {
        let access = <Query<'_, Pos> as SystemParam>::access();
        assert_eq!(access, vec![Access::CompRead(TypeId::of::<Pos>())]);
    }

    #[test]
    fn query_mut_allows_mutation() {
        let mut world = World::new();
        let e = world.spawn((Pos { x: 5.0 },));
        let cell = unsafe { UnsafeWorldCell::new(&mut world as *mut World) };
        unsafe {
            let mut q: QueryMut<'_, Pos> = <QueryMut<'_, Pos> as SystemParam>::fetch(cell);
            for (_, pos) in q.iter_mut() {
                pos.x += 10.0;
            }
        }
        assert_eq!(world.get::<Pos>(e).unwrap().x, 15.0);
    }

    #[test]
    fn query_mut_access_is_comp_write() {
        let access = <QueryMut<'_, Pos> as SystemParam>::access();
        assert_eq!(access, vec![Access::CompWrite(TypeId::of::<Pos>())]);
    }

    #[test]
    fn query_empty_world() {
        let mut world = World::new();
        let cell = unsafe { UnsafeWorldCell::new(&mut world as *mut World) };
        unsafe {
            let q: Query<'_, Pos> = <Query<'_, Pos> as SystemParam>::fetch(cell);
            assert!(q.is_empty());
        }
    }

    // -- Tuple expansion tests --

    #[test]
    fn unit_tuple_has_no_access() {
        let access = <() as SystemParam>::access();
        assert!(access.is_empty());
    }

    #[test]
    fn pair_tuple_aggregates_access() {
        let access = <(Res<'_, i32>, ResMut<'_, u32>) as SystemParam>::access();
        assert_eq!(access.len(), 2);
        assert!(access.contains(&Access::ResRead(TypeId::of::<i32>())));
        assert!(access.contains(&Access::ResWrite(TypeId::of::<u32>())));
    }

    #[test]
    fn triple_tuple_aggregates_access() {
        let access = <(Res<'_, i32>, ResMut<'_, u32>, Query<'_, Pos>) as SystemParam>::access();
        assert_eq!(access.len(), 3);
    }

    // -- Missing resource panic test (#58) --

    #[test]
    #[should_panic(expected = "resource not found")]
    fn res_fetch_panics_on_missing_resource() {
        let mut world = World::new();
        // Do NOT insert any i32 resource.
        let cell = unsafe { UnsafeWorldCell::new(&mut world as *mut World) };
        unsafe {
            let _: Res<'_, i32> = <Res<'_, i32> as SystemParam>::fetch(cell);
        }
    }

    // -- QueryMut on empty world (#58) --

    #[test]
    fn query_mut_empty_world() {
        let mut world = World::new();
        let cell = unsafe { UnsafeWorldCell::new(&mut world as *mut World) };
        unsafe {
            let q: QueryMut<'_, Pos> = <QueryMut<'_, Pos> as SystemParam>::fetch(cell);
            assert!(q.is_empty());
        }
    }

    // -- 4+ arity smoke test (#58) --

    #[derive(Debug, PartialEq)]
    struct Vel {
        y: f32,
    }
    impl Component for Vel {}

    struct TimeRes(f32);
    struct GravRes(f32);

    #[test]
    fn four_arity_tuple_access_and_fetch() {
        let mut world = World::new();
        world.insert_resource(TimeRes(1.0));
        world.insert_resource(GravRes(9.8));
        world.spawn((Pos { x: 0.0 },));
        world.spawn((Vel { y: 0.0 },));

        // Verify access aggregation for 4-param tuple.
        let access = <(
            Res<'_, TimeRes>,
            Res<'_, GravRes>,
            Query<'_, Pos>,
            Query<'_, Vel>,
        ) as SystemParam>::access();
        assert_eq!(access.len(), 4);

        // Verify fetch works.
        let cell = unsafe { UnsafeWorldCell::new(&mut world as *mut World) };
        unsafe {
            let (time, grav, positions, velocities) = <(
                Res<'_, TimeRes>,
                Res<'_, GravRes>,
                Query<'_, Pos>,
                Query<'_, Vel>,
            ) as SystemParam>::fetch(cell);
            assert!((time.0 - 1.0).abs() < f32::EPSILON);
            assert!((grav.0 - 9.8).abs() < f32::EPSILON);
            assert_eq!(positions.len(), 1);
            assert_eq!(velocities.len(), 1);
        }
    }

    // -- Query + QueryMut combo on different types (#58) --

    #[test]
    fn query_read_and_query_mut_different_types() {
        let mut world = World::new();
        world.spawn((Pos { x: 1.0 }, Vel { y: 2.0 }));
        world.spawn((Pos { x: 3.0 }, Vel { y: 4.0 }));

        // No conflict: CompRead(Pos) + CompWrite(Vel).
        let a = <Query<'_, Pos> as SystemParam>::access();
        let b = <QueryMut<'_, Vel> as SystemParam>::access();
        assert!(!has_conflicts(&a, &b));

        // Fetch both simultaneously.
        let cell = unsafe { UnsafeWorldCell::new(&mut world as *mut World) };
        unsafe {
            let positions: Query<'_, Pos> = <Query<'_, Pos> as SystemParam>::fetch(cell);
            let mut velocities: QueryMut<'_, Vel> = <QueryMut<'_, Vel> as SystemParam>::fetch(cell);

            assert_eq!(positions.len(), 2);
            assert_eq!(velocities.len(), 2);

            // Mutate velocities while positions are live — the soundness scenario.
            for (_, v) in velocities.iter_mut() {
                v.y += 10.0;
            }
        }

        // Verify mutations applied.
        let ys: Vec<f32> = world.query::<&Vel>().map(|(_, v)| v.y).collect();
        assert!(ys.iter().all(|&y| y > 10.0));
    }
}
