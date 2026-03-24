// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

use crate::component::{Component, ComponentStorage, TypedSparseSet};
use crate::entity::{Entity, EntityAllocator};
use crate::query::{Query2Iter, QueryIter, QueryIterMut};
use crate::resource::Resources;

/// A bundle of components that can be spawned together.
///
/// Implemented for tuples of components up to 8 elements.
pub trait Bundle {
    #[doc(hidden)]
    fn insert_into(self, storage: &mut ComponentStorage, entity_index: u32);
}

// Implement Bundle for single component.
impl<A: Component> Bundle for (A,) {
    fn insert_into(self, storage: &mut ComponentStorage, entity_index: u32) {
        storage.typed_set_mut::<A>().insert(entity_index, self.0);
    }
}

// Implement Bundle for tuples of 2–8 components via macro.
macro_rules! impl_bundle {
    ($($t:ident),+) => {
        #[allow(non_snake_case)]
        impl<$($t: Component),+> Bundle for ($($t,)+) {
            fn insert_into(self, storage: &mut ComponentStorage, entity_index: u32) {
                let ($($t,)+) = self;
                $(storage.typed_set_mut::<$t>().insert(entity_index, $t);)+
            }
        }
    };
}

impl_bundle!(A, B);
impl_bundle!(A, B, C);
impl_bundle!(A, B, C, D);
impl_bundle!(A, B, C, D, E);
impl_bundle!(A, B, C, D, E, F);
impl_bundle!(A, B, C, D, E, F, G);
impl_bundle!(A, B, C, D, E, F, G, H);

/// The ECS world: owns entities, component storage, and resources.
pub struct World {
    entities: EntityAllocator,
    components: ComponentStorage,
    resources: Resources,
}

impl World {
    /// Create an empty world.
    pub fn new() -> Self {
        Self {
            entities: EntityAllocator::new(),
            components: ComponentStorage::new(),
            resources: Resources::new(),
        }
    }

    /// Spawn an entity with the given component bundle.
    pub fn spawn<B: Bundle>(&mut self, bundle: B) -> Entity {
        let entity = self.entities.alloc();
        bundle.insert_into(&mut self.components, entity.index);
        entity
    }

    /// Despawn an entity, removing all its components.
    pub fn despawn(&mut self, entity: Entity) -> bool {
        if self.entities.dealloc(entity) {
            self.components.remove_all(entity.index);
            true
        } else {
            false
        }
    }

    /// Check whether an entity is alive.
    pub fn is_alive(&self, entity: Entity) -> bool {
        self.entities.is_alive(entity)
    }

    /// Insert a resource (world-global singleton).
    pub fn insert_resource<T: 'static>(&mut self, value: T) {
        self.resources.insert(value);
    }

    /// Get a reference to a resource. Panics if not present.
    pub fn resource<T: 'static>(&self) -> &T {
        self.resources.get::<T>()
    }

    /// Get a mutable reference to a resource. Panics if not present.
    pub fn resource_mut<T: 'static>(&mut self) -> &mut T {
        self.resources.get_mut::<T>()
    }

    /// Try to get a reference to a resource. Returns `None` if not present.
    pub fn try_resource<T: 'static>(&self) -> Option<&T> {
        self.resources.try_get::<T>()
    }

    /// Remove and return a resource. Panics if not present.
    pub fn take_resource<T: 'static>(&mut self) -> T {
        self.resources.take::<T>()
    }

    /// Try to remove and return a resource. Returns `None` if not present.
    pub fn try_take_resource<T: 'static>(&mut self) -> Option<T> {
        self.resources.try_take::<T>()
    }

    /// Get a component for an entity.
    pub fn get<T: Component>(&self, entity: Entity) -> Option<&T> {
        if !self.entities.is_alive(entity) {
            return None;
        }
        self.components.typed_set::<T>()?.get(entity.index)
    }

    /// Get a mutable component for an entity.
    pub fn get_mut<T: Component>(&mut self, entity: Entity) -> Option<&mut T> {
        if !self.entities.is_alive(entity) {
            return None;
        }
        self.components.typed_set_mut::<T>().get_mut(entity.index)
    }

    /// Query all entities that have component T (immutable).
    ///
    /// Returns a lazy iterator of `(Entity, &T)` pairs — no heap allocation.
    pub fn query<T: Component>(&self) -> QueryIter<'_, T> {
        let Some(set) = self.components.typed_set::<T>() else {
            return QueryIter::new(&self.entities, &[], &[]);
        };
        let (dense, data) = set.dense_data();
        QueryIter::new(&self.entities, dense, data)
    }

    /// Query all entities that have component T (mutable).
    ///
    /// Returns a lazy iterator of `(Entity, &mut T)` pairs — no heap allocation.
    pub fn query_mut<T: Component>(&mut self) -> QueryIterMut<'_, T> {
        let entities = &self.entities;
        let set = self.components.typed_set_mut::<T>();
        let (dense, data) = set.dense_data_mut();
        QueryIterMut::new(entities, dense, data)
    }

    /// Query all entities with two components (both immutable).
    ///
    /// Returns a lazy iterator — iterates set A, probes set B.
    pub fn query2<A: Component, B: Component>(&self) -> Query2Iter<'_, A, B> {
        let (Some(set_a), Some(set_b)) = (
            self.components.typed_set::<A>(),
            self.components.typed_set::<B>(),
        ) else {
            return Query2Iter::empty(&self.entities);
        };
        let (dense_a, data_a) = set_a.dense_data();
        Query2Iter::new(&self.entities, dense_a, data_a, set_b)
    }

    /// Query all entities with two components (both mutable).
    pub fn query2_mut<A: Component, B: Component>(&mut self) -> Vec<(Entity, &mut A, &mut B)> {
        let entities = &self.entities;
        let (set_a, set_b) = self.components.typed_sets_two_mut::<A, B>();
        let (Some(sa), Some(sb)) = (set_a, set_b) else {
            return Vec::new();
        };

        sa.iter_mut()
            .filter_map(|(idx, a)| {
                // SAFETY: sa and sb are distinct typed sparse sets (enforced by
                // typed_sets_two_mut's TypeId assertion). Each entity index maps
                // to a unique dense slot, so repeated get_mut calls never alias.
                let sb_ptr = sb as *mut TypedSparseSet<B>;
                let b = unsafe { (*sb_ptr).get_mut(idx)? };
                Some((entities.entity_at(idx)?, a, b))
            })
            .collect()
    }

    /// Returns the number of alive entities.
    pub fn entity_count(&self) -> usize {
        self.entities.alive_entities().count()
    }
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq)]
    struct Pos {
        x: f32,
        y: f32,
    }
    impl Component for Pos {}

    #[derive(Debug, Clone, PartialEq)]
    struct Vel {
        x: f32,
        y: f32,
    }
    impl Component for Vel {}

    #[derive(Debug, Clone)]
    struct Health(i32);
    impl Component for Health {}

    #[test]
    fn spawn_and_get() {
        let mut world = World::new();
        let e = world.spawn((Pos { x: 1.0, y: 2.0 },));
        assert!(world.is_alive(e));
        let pos = world.get::<Pos>(e).unwrap();
        assert_eq!(pos.x, 1.0);
        assert_eq!(pos.y, 2.0);
    }

    #[test]
    fn spawn_multi_component() {
        let mut world = World::new();
        let e = world.spawn((Pos { x: 0.0, y: 0.0 }, Vel { x: 1.0, y: 2.0 }));
        assert!(world.get::<Pos>(e).is_some());
        assert!(world.get::<Vel>(e).is_some());
    }

    #[test]
    fn despawn_removes_entity_and_components() {
        let mut world = World::new();
        let e = world.spawn((Pos { x: 0.0, y: 0.0 }, Health(100)));
        assert!(world.despawn(e));
        assert!(!world.is_alive(e));
        assert!(world.get::<Pos>(e).is_none());
        assert!(world.get::<Health>(e).is_none());
    }

    #[test]
    fn query_iterates_matching_entities() {
        let mut world = World::new();
        world.spawn((Pos { x: 1.0, y: 0.0 },));
        world.spawn((Pos { x: 2.0, y: 0.0 }, Vel { x: 0.0, y: 0.0 }));
        world.spawn((Vel { x: 3.0, y: 0.0 },)); // no Pos

        let positions: Vec<f32> = world.query::<Pos>().map(|(_, p)| p.x).collect();
        assert_eq!(positions.len(), 2);
        assert!(positions.contains(&1.0));
        assert!(positions.contains(&2.0));
    }

    #[test]
    fn query_mut_allows_modification() {
        let mut world = World::new();
        world.spawn((Pos { x: 0.0, y: 0.0 },));
        world.spawn((Pos { x: 10.0, y: 10.0 },));

        for (_, pos) in world.query_mut::<Pos>() {
            pos.x += 1.0;
        }

        let xs: Vec<f32> = world.query::<Pos>().map(|(_, p)| p.x).collect();
        assert!(xs.contains(&1.0));
        assert!(xs.contains(&11.0));
    }

    #[test]
    fn resources() {
        let mut world = World::new();

        struct DeltaTime(f64);

        world.insert_resource(DeltaTime(0.016));
        assert_eq!(world.resource::<DeltaTime>().0, 0.016);

        world.resource_mut::<DeltaTime>().0 = 0.032;
        assert_eq!(world.resource::<DeltaTime>().0, 0.032);
    }

    #[test]
    fn entity_count() {
        let mut world = World::new();
        assert_eq!(world.entity_count(), 0);
        let e1 = world.spawn((Pos { x: 0.0, y: 0.0 },));
        let _e2 = world.spawn((Pos { x: 0.0, y: 0.0 },));
        assert_eq!(world.entity_count(), 2);
        world.despawn(e1);
        assert_eq!(world.entity_count(), 1);
    }

    #[test]
    fn query2_returns_entities_with_both_components() {
        let mut world = World::new();
        world.spawn((Pos { x: 1.0, y: 0.0 },));
        world.spawn((Pos { x: 2.0, y: 0.0 }, Vel { x: 5.0, y: 0.0 }));
        world.spawn((Vel { x: 3.0, y: 0.0 },));

        let results: Vec<_> = world.query2::<Pos, Vel>().collect();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1.x, 2.0);
        assert_eq!(results[0].2.x, 5.0);
    }

    #[test]
    fn query2_mut_mutates_both_components() {
        let mut world = World::new();
        let e1 = world.spawn((Pos { x: 1.0, y: 1.0 }, Vel { x: 10.0, y: 10.0 }));
        let e2 = world.spawn((Pos { x: 2.0, y: 2.0 }, Vel { x: 20.0, y: 20.0 }));
        let e3 = world.spawn((Pos { x: 3.0, y: 3.0 }, Vel { x: 30.0, y: 30.0 }));

        for (_, pos, vel) in world.query2_mut::<Pos, Vel>() {
            pos.x += 100.0;
            vel.y += 200.0;
        }

        assert_eq!(world.get::<Pos>(e1).unwrap().x, 101.0);
        assert_eq!(world.get::<Vel>(e1).unwrap().y, 210.0);
        assert_eq!(world.get::<Pos>(e2).unwrap().x, 102.0);
        assert_eq!(world.get::<Vel>(e2).unwrap().y, 220.0);
        assert_eq!(world.get::<Pos>(e3).unwrap().x, 103.0);
        assert_eq!(world.get::<Vel>(e3).unwrap().y, 230.0);
    }

    #[test]
    fn query2_mut_skips_entities_missing_one_component() {
        let mut world = World::new();
        let e1 = world.spawn((Pos { x: 5.0, y: 5.0 },));
        let e2 = world.spawn((Pos { x: 7.0, y: 7.0 }, Vel { x: 9.0, y: 9.0 }));
        let e3 = world.spawn((Vel { x: 11.0, y: 11.0 },));

        let results = world.query2_mut::<Pos, Vel>();
        assert_eq!(results.len(), 1);

        let (entity, pos, vel) = &results[0];
        assert_eq!(*entity, e2);
        assert_eq!(pos.x, 7.0);
        assert_eq!(vel.x, 9.0);

        // e1's Pos must be unchanged.
        assert_eq!(world.get::<Pos>(e1).unwrap().x, 5.0);
        // e3's Vel must be unchanged.
        assert_eq!(world.get::<Vel>(e3).unwrap().x, 11.0);
    }

    #[test]
    #[should_panic(expected = "cannot borrow the same sparse set mutably twice")]
    fn query2_mut_same_type_panics() {
        let mut world = World::new();
        world.spawn((Pos { x: 0.0, y: 0.0 },));
        world.query2_mut::<Pos, Pos>();
    }
}
