// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

use std::any::TypeId;
use std::ops::{Deref, DerefMut};
use crate::world::World;
use crate::component::Component;
use crate::entity::Entity;

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
    /// parallel system execution.
    ///
    /// Conflict rules:
    /// - `ResRead`  + `ResWrite`  (same TypeId) → conflict
    /// - `ResWrite` + `ResWrite`  (same TypeId) → conflict
    /// - `CompRead` + `CompWrite` (same TypeId) → conflict
    /// - `CompWrite`+ `CompWrite` (same TypeId) → conflict
    /// - Everything else → no conflict (read-read, different TypeIds, cross-namespace)
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

/// Returns `true` if any access in slice `a` conflicts with any access in
/// slice `b`.
pub fn has_conflicts(a: &[Access], b: &[Access]) -> bool {
    a.iter().any(|x| b.iter().any(|y| x.conflicts_with(y)))
}

/// A type that can be extracted from a `World` as a system parameter.
///
/// # Safety
///
/// Implementations must correctly report all data accessed via `access()`.
/// `fetch()` may only touch the data declared in `access()`. The caller
/// guarantees that no other parameter has aliasing mutable access to the same
/// data — enforced at system registration time by conflict detection.
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
    /// in `access()`. Each resource/component lives in its own heap
    /// allocation (Box inside HashMap), so accesses to different TypeIds do
    /// not alias even through the same `*mut World`.
    unsafe fn fetch<'w>(world: *mut World) -> Self::Item<'w>;
}

pub struct Res<'w, T: 'static> {
    value: &'w T,
}

impl<T: 'static> Deref for Res<'_, T> {
    type Target = T;
    fn deref(&self) -> &T { self.value }
}

// SAFETY: access() correctly reports ResRead. fetch() only reads the resource.
unsafe impl<T: 'static> SystemParam for Res<'_, T> {
    type Item<'w> = Res<'w, T>;
    fn access() -> Vec<Access> { vec![Access::ResRead(TypeId::of::<T>())] }
    unsafe fn fetch<'w>(world: *mut World) -> Res<'w, T> {
        Res { value: unsafe { (*world).resource::<T>() } }
    }
}

pub struct ResMut<'w, T: 'static> {
    value: &'w mut T,
}

impl<T: 'static> Deref for ResMut<'_, T> {
    type Target = T;
    fn deref(&self) -> &T { self.value }
}

impl<T: 'static> DerefMut for ResMut<'_, T> {
    fn deref_mut(&mut self) -> &mut T { self.value }
}

// SAFETY: access() correctly reports ResWrite. fetch() only mutates this resource.
unsafe impl<T: 'static> SystemParam for ResMut<'_, T> {
    type Item<'w> = ResMut<'w, T>;
    fn access() -> Vec<Access> { vec![Access::ResWrite(TypeId::of::<T>())] }
    unsafe fn fetch<'w>(world: *mut World) -> ResMut<'w, T> {
        ResMut { value: unsafe { (*world).resource_mut::<T>() } }
    }
}

pub struct Query<'w, T: Component> {
    results: Vec<(Entity, &'w T)>,
}

impl<'w, T: Component> Query<'w, T> {
    pub fn iter(&self) -> impl Iterator<Item = (Entity, &T)> {
        self.results.iter().map(|&(e, v)| (e, v))
    }
    pub fn is_empty(&self) -> bool { self.results.is_empty() }
    pub fn len(&self) -> usize { self.results.len() }
}

// SAFETY: access() correctly reports CompRead.
unsafe impl<T: Component> SystemParam for Query<'_, T> {
    type Item<'w> = Query<'w, T>;
    fn access() -> Vec<Access> { vec![Access::CompRead(TypeId::of::<T>())] }
    unsafe fn fetch<'w>(world: *mut World) -> Query<'w, T> {
        Query { results: unsafe { (*world).query::<T>() } }
    }
}

pub struct QueryMut<'w, T: Component> {
    results: Vec<(Entity, &'w mut T)>,
}

impl<'w, T: Component> QueryMut<'w, T> {
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (Entity, &mut T)> + '_ {
        self.results.iter_mut().map(|(e, v)| (*e, &mut **v))
    }
    pub fn is_empty(&self) -> bool { self.results.is_empty() }
    pub fn len(&self) -> usize { self.results.len() }
}

// SAFETY: access() correctly reports CompWrite.
unsafe impl<T: Component> SystemParam for QueryMut<'_, T> {
    type Item<'w> = QueryMut<'w, T>;
    fn access() -> Vec<Access> { vec![Access::CompWrite(TypeId::of::<T>())] }
    unsafe fn fetch<'w>(world: *mut World) -> QueryMut<'w, T> {
        QueryMut { results: unsafe { (*world).query_mut::<T>() } }
    }
}

// SAFETY: No access, no fetch.
unsafe impl SystemParam for () {
    type Item<'w> = ();
    fn access() -> Vec<Access> { Vec::new() }
    unsafe fn fetch<'w>(_world: *mut World) -> Self::Item<'w> {}
}

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
            unsafe fn fetch<'w>(world: *mut World) -> Self::Item<'w> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::World;
    use crate::component::Component;

    fn type_id<T: 'static>() -> TypeId {
        TypeId::of::<T>()
    }

    #[derive(Debug, PartialEq)]
    struct Pos { x: f32 }
    impl Component for Pos {}

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
        // symmetric
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
        // symmetric
        assert!(b.conflicts_with(&a));
    }

    #[test]
    fn res_and_comp_same_type_no_conflict() {
        // Cross-namespace: ResWrite and CompWrite with the identical TypeId are
        // independent namespaces and must not conflict.
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
            Access::ResWrite(type_id::<u32>()), // conflicts with ResRead(u32) in set_a
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

    // Task 2 tests: SystemParam trait + Res<T> + ResMut<T>
    #[test]
    fn res_fetches_resource() {
        let mut world = World::new();
        world.insert_resource(42_i32);
        let world_ptr = &mut world as *mut World;
        unsafe {
            let res: Res<'_, i32> = <Res<'_, i32> as SystemParam>::fetch(world_ptr);
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
        let world_ptr = &mut world as *mut World;
        unsafe {
            let mut res: ResMut<'_, u32> = <ResMut<'_, u32> as SystemParam>::fetch(world_ptr);
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

    // Task 3 tests: Query<T> and QueryMut<T>
    #[test]
    fn query_fetches_matching_entities() {
        let mut world = World::new();
        world.spawn((Pos { x: 1.0 },));
        world.spawn((Pos { x: 2.0 },));
        let world_ptr = &mut world as *mut World;
        unsafe {
            let q: Query<'_, Pos> = <Query<'_, Pos> as SystemParam>::fetch(world_ptr);
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
        let world_ptr = &mut world as *mut World;
        unsafe {
            let mut q: QueryMut<'_, Pos> = <QueryMut<'_, Pos> as SystemParam>::fetch(world_ptr);
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
        let world_ptr = &mut world as *mut World;
        unsafe {
            let q: Query<'_, Pos> = <Query<'_, Pos> as SystemParam>::fetch(world_ptr);
            assert!(q.is_empty());
        }
    }

    // Task 4 tests: Tuple SystemParam impls
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
}
