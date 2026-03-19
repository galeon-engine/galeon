// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

use crate::component::{Component, ComponentStorage, TypedSparseSet};
use crate::entity::{Entity, EntityAllocator};
use crate::resource::Resources;

/// A bundle of components that can be spawned together.
///
/// Implemented for tuples of components up to 8 elements.
pub trait Bundle {
    #[doc(hidden)]
    fn insert_into(
        self,
        storage: &mut ComponentStorage,
        entity_index: u32,
        tick: u64,
        change_cursor: u64,
    );
}

// Implement Bundle for single component.
impl<A: Component> Bundle for (A,) {
    fn insert_into(
        self,
        storage: &mut ComponentStorage,
        entity_index: u32,
        tick: u64,
        change_cursor: u64,
    ) {
        storage
            .typed_set_mut::<A>()
            .insert(entity_index, self.0, tick, change_cursor);
    }
}

// Implement Bundle for tuples of 2–8 components via macro.
macro_rules! impl_bundle {
    ($($t:ident),+) => {
        #[allow(non_snake_case)]
        impl<$($t: Component),+> Bundle for ($($t,)+) {
            fn insert_into(
                self,
                storage: &mut ComponentStorage,
                entity_index: u32,
                tick: u64,
                change_cursor: u64,
            ) {
                let ($($t,)+) = self;
                $(storage.typed_set_mut::<$t>().insert(entity_index, $t, tick, change_cursor);)+
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
    tick: u64,
    change_cursor: u64,
}

impl World {
    /// Create an empty world.
    pub fn new() -> Self {
        Self {
            entities: EntityAllocator::new(),
            components: ComponentStorage::new(),
            resources: Resources::new(),
            tick: 1,
            change_cursor: 0,
        }
    }

    /// Returns the current ECS tick. Starts at 1, advances each schedule run.
    pub fn current_tick(&self) -> u64 {
        self.tick
    }

    /// Returns the current monotonic change cursor for incremental consumers.
    pub fn current_change_cursor(&self) -> u64 {
        self.change_cursor
    }

    /// Advance the tick counter. Called at the start of each schedule run.
    pub(crate) fn advance_tick(&mut self) {
        self.tick += 1;
    }

    fn bump_change_cursor(&mut self) -> u64 {
        self.change_cursor += 1;
        self.change_cursor
    }

    /// Spawn an entity with the given component bundle.
    pub fn spawn<B: Bundle>(&mut self, bundle: B) -> Entity {
        let entity = self.entities.alloc();
        let tick = self.tick;
        let change_cursor = self.bump_change_cursor();
        bundle.insert_into(&mut self.components, entity.index, tick, change_cursor);
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
        let tick = self.tick;
        let change_cursor = self.bump_change_cursor();
        self.components
            .typed_set_existing_mut::<T>()?
            .get_mut(entity.index, tick, change_cursor)
    }

    /// Returns the tick at which a component was added to an entity.
    pub fn component_added_tick<T: Component>(&self, entity: Entity) -> Option<u64> {
        if !self.entities.is_alive(entity) {
            return None;
        }
        self.components.typed_set::<T>()?.added_tick(entity.index)
    }

    /// Returns the tick at which a component was last changed on an entity.
    pub fn component_changed_tick<T: Component>(&self, entity: Entity) -> Option<u64> {
        if !self.entities.is_alive(entity) {
            return None;
        }
        self.components.typed_set::<T>()?.changed_tick(entity.index)
    }

    /// Returns the change cursor for the last mutation on a component.
    pub fn component_changed_cursor<T: Component>(&self, entity: Entity) -> Option<u64> {
        if !self.entities.is_alive(entity) {
            return None;
        }
        self.components
            .typed_set::<T>()?
            .changed_cursor(entity.index)
    }

    /// Query all entities that have component T (immutable).
    ///
    /// Returns an iterator of `(Entity, &T)` pairs.
    pub fn query<T: Component>(&self) -> Vec<(Entity, &T)> {
        let Some(set) = self.components.typed_set::<T>() else {
            return Vec::new();
        };
        set.iter()
            .filter_map(|(idx, val)| Some((self.entities.entity_at(idx)?, val)))
            .collect()
    }

    /// Query all entities that have component T (mutable).
    ///
    /// Returns a `Vec` since we can't return iterators over `&mut` with
    /// the current storage model without GATs or complex lifetime tricks.
    ///
    /// Marks ALL components in the set as changed at the current tick.
    pub fn query_mut<T: Component>(&mut self) -> Vec<(Entity, &mut T)> {
        let tick = self.tick;
        let change_cursor = self.bump_change_cursor();
        let entities = &self.entities;
        let Some(set) = self.components.typed_set_existing_mut::<T>() else {
            return Vec::new();
        };
        set.mark_all_changed(tick, change_cursor);
        set.iter_mut()
            .filter_map(|(idx, val)| Some((entities.entity_at(idx)?, val)))
            .collect()
    }

    /// Returns entities whose component `T` has a `changed_tick > since`.
    /// Use `since: 0` to get all components (sentinel).
    pub fn query_changed<T: Component>(&self, since: u64) -> Vec<(Entity, &T)> {
        let entities = &self.entities;
        let Some(set) = self.components.typed_set::<T>() else {
            return Vec::new();
        };
        set.iter_changed(since)
            .filter_map(|(idx, data)| {
                let entity = entities.entity_at(idx)?;
                Some((entity, data))
            })
            .collect()
    }

    /// Returns entities whose component `T` has an `added_tick > since`.
    /// Use `since: 0` to get all components (sentinel).
    pub fn query_added<T: Component>(&self, since: u64) -> Vec<(Entity, &T)> {
        let entities = &self.entities;
        let Some(set) = self.components.typed_set::<T>() else {
            return Vec::new();
        };
        set.iter_added(since)
            .filter_map(|(idx, data)| {
                let entity = entities.entity_at(idx)?;
                Some((entity, data))
            })
            .collect()
    }

    /// Query all entities with two components (both immutable).
    pub fn query2<A: Component, B: Component>(&self) -> Vec<(Entity, &A, &B)> {
        let (Some(set_a), Some(set_b)) = (
            self.components.typed_set::<A>(),
            self.components.typed_set::<B>(),
        ) else {
            return Vec::new();
        };

        // Iterate the first set and probe the second.
        set_a
            .iter()
            .filter_map(|(idx, a)| {
                let b = set_b.get(idx)?;
                Some((self.entities.entity_at(idx)?, a, b))
            })
            .collect()
    }

    /// Query all entities with two components (both mutable).
    pub fn query2_mut<A: Component, B: Component>(&mut self) -> Vec<(Entity, &mut A, &mut B)> {
        let tick = self.tick;
        let change_cursor = self.bump_change_cursor();
        let (set_a, set_b) = self.components.typed_sets_two_mut::<A, B>();
        let (Some(sa), Some(sb)) = (set_a, set_b) else {
            return Vec::new();
        };

        let entities = &self.entities;
        let indices: Vec<u32> = sa.entity_indices().collect();
        let mut result = Vec::new();
        let sa_ptr: *mut TypedSparseSet<A> = sa;
        let sb_ptr: *mut TypedSparseSet<B> = sb;
        for idx in indices {
            if !unsafe { (&*sb_ptr).contains(idx) } {
                continue;
            }
            let Some(entity) = entities.entity_at(idx) else {
                continue;
            };
            // SAFETY: sa and sb are distinct typed sparse sets backed by
            // different TypeIds (enforced by typed_sets_two_mut's TypeId
            // assertion). The loop only borrows one entity slot per set at a
            // time, and we gate on `sb.contains(idx)` before taking mutable
            // references so unmatched `A` rows are not marked as changed.
            let a = unsafe { (&mut *sa_ptr).get_mut(idx, tick, change_cursor) }
                .expect("entity index came from set_a");
            let b = unsafe { (&mut *sb_ptr).get_mut(idx, tick, change_cursor) }
                .expect("entity index was confirmed in set_b");
            result.push((entity, a, b));
        }
        result
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

        let positions: Vec<f32> = world.query::<Pos>().iter().map(|(_, p)| p.x).collect();
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

        let xs: Vec<f32> = world.query::<Pos>().iter().map(|(_, p)| p.x).collect();
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

        let results = world.query2::<Pos, Vel>();
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
    fn query2_mut_only_marks_intersection_changed() {
        let mut world = World::new();
        let e1 = world.spawn((Pos { x: 5.0, y: 5.0 },));
        let e2 = world.spawn((Pos { x: 7.0, y: 7.0 }, Vel { x: 9.0, y: 9.0 }));
        let e3 = world.spawn((Vel { x: 11.0, y: 11.0 },));

        world.advance_tick();
        let _ = world.query2_mut::<Pos, Vel>();

        let changed_pos: Vec<_> = world
            .query_changed::<Pos>(1)
            .into_iter()
            .map(|(entity, _)| entity)
            .collect();
        assert_eq!(changed_pos, vec![e2]);

        let changed_vel: Vec<_> = world
            .query_changed::<Vel>(1)
            .into_iter()
            .map(|(entity, _)| entity)
            .collect();
        assert_eq!(changed_vel, vec![e2]);

        assert_eq!(world.component_changed_tick::<Pos>(e1), Some(1));
        assert_eq!(world.component_changed_tick::<Pos>(e2), Some(2));
        assert_eq!(world.component_changed_tick::<Vel>(e3), Some(1));
    }

    #[test]
    #[should_panic(expected = "cannot borrow the same sparse set mutably twice")]
    fn query2_mut_same_type_panics() {
        let mut world = World::new();
        world.spawn((Pos { x: 0.0, y: 0.0 },));
        world.query2_mut::<Pos, Pos>();
    }
}
