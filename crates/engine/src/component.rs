// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

use std::any::{Any, TypeId};
use std::collections::HashMap;

/// Marker trait for types that can be stored as ECS components.
///
/// Derive with `#[derive(Component)]` from `galeon_engine_macros`.
pub trait Component: 'static {}

// =============================================================================
// TypedSparseSet<T> — typed, cache-friendly component storage
// =============================================================================

/// Typed storage for a single component type, backed by a sparse set.
///
/// Stores components in a dense `Vec<T>` — no boxing, no runtime downcasts.
/// Sparse set gives O(1) insert/get/remove and dense iteration — ideal for
/// RTS entities that frequently gain/lose components.
pub(crate) struct TypedSparseSet<T> {
    /// Sparse array: entity index → dense index (or `u32::MAX` if absent).
    sparse: Vec<u32>,
    /// Dense array of entity indices that have this component.
    dense: Vec<u32>,
    /// Parallel to `dense`: the actual component data (typed, no Box).
    data: Vec<T>,
    /// Tick at which each component instance was first inserted. Parallel to `dense`.
    added_ticks: Vec<u64>,
    /// Tick at which each component instance was last mutated. Parallel to `dense`.
    changed_ticks: Vec<u64>,
    /// Monotonic change cursor for insertion. Parallel to `dense`.
    added_cursors: Vec<u64>,
    /// Monotonic change cursor for mutation. Parallel to `dense`.
    changed_cursors: Vec<u64>,
}

impl<T> TypedSparseSet<T> {
    pub fn new() -> Self {
        Self {
            sparse: Vec::new(),
            dense: Vec::new(),
            data: Vec::new(),
            added_ticks: Vec::new(),
            changed_ticks: Vec::new(),
            added_cursors: Vec::new(),
            changed_cursors: Vec::new(),
        }
    }

    /// Insert a component for an entity. Overwrites if already present.
    ///
    /// On first insert, both `added_tick` and `changed_tick` are set to `tick`.
    /// On overwrite, only `changed_tick` is updated — `added_tick` is preserved.
    pub fn insert(&mut self, entity_index: u32, value: T, tick: u64, cursor: u64) {
        let idx = entity_index as usize;

        // Grow sparse array if needed.
        if idx >= self.sparse.len() {
            self.sparse.resize(idx + 1, u32::MAX);
        }

        if self.sparse[idx] != u32::MAX {
            // Overwrite existing — update data and changed_tick only.
            let dense_idx = self.sparse[idx] as usize;
            self.data[dense_idx] = value;
            self.changed_ticks[dense_idx] = tick;
            self.changed_cursors[dense_idx] = cursor;
        } else {
            // New entry.
            let dense_idx = self.dense.len() as u32;
            self.sparse[idx] = dense_idx;
            self.dense.push(entity_index);
            self.data.push(value);
            self.added_ticks.push(tick);
            self.changed_ticks.push(tick);
            self.added_cursors.push(cursor);
            self.changed_cursors.push(cursor);
        }
    }

    /// Get a reference to the component for an entity.
    pub fn get(&self, entity_index: u32) -> Option<&T> {
        let idx = entity_index as usize;
        if idx < self.sparse.len() && self.sparse[idx] != u32::MAX {
            let dense_idx = self.sparse[idx] as usize;
            Some(&self.data[dense_idx])
        } else {
            None
        }
    }

    /// Get a mutable reference to the component for an entity.
    ///
    /// Stamps `changed_tick` with `tick` unconditionally.
    pub fn get_mut(&mut self, entity_index: u32, tick: u64, cursor: u64) -> Option<&mut T> {
        let idx = entity_index as usize;
        if idx < self.sparse.len() && self.sparse[idx] != u32::MAX {
            let dense_idx = self.sparse[idx] as usize;
            self.changed_ticks[dense_idx] = tick;
            self.changed_cursors[dense_idx] = cursor;
            Some(&mut self.data[dense_idx])
        } else {
            None
        }
    }

    /// Remove the component for an entity. Returns `true` if it was present.
    pub fn remove(&mut self, entity_index: u32) -> bool {
        let idx = entity_index as usize;
        if idx >= self.sparse.len() || self.sparse[idx] == u32::MAX {
            return false;
        }

        let dense_idx = self.sparse[idx] as usize;
        let last_dense = self.dense.len() - 1;

        // Clear sparse entry for removed entity.
        self.sparse[idx] = u32::MAX;

        if dense_idx != last_dense {
            // Swap with last element.
            let swapped_entity = self.dense[last_dense];
            self.dense[dense_idx] = swapped_entity;
            self.data.swap(dense_idx, last_dense);
            self.added_ticks.swap(dense_idx, last_dense);
            self.changed_ticks.swap(dense_idx, last_dense);
            self.added_cursors.swap(dense_idx, last_dense);
            self.changed_cursors.swap(dense_idx, last_dense);
            self.sparse[swapped_entity as usize] = dense_idx as u32;
        }

        self.dense.pop();
        self.data.pop();
        self.added_ticks.pop();
        self.changed_ticks.pop();
        self.added_cursors.pop();
        self.changed_cursors.pop();

        true
    }

    /// Returns the tick at which this component was first inserted for the entity.
    pub fn added_tick(&self, entity_index: u32) -> Option<u64> {
        let idx = entity_index as usize;
        if idx >= self.sparse.len() || self.sparse[idx] == u32::MAX {
            return None;
        }
        Some(self.added_ticks[self.sparse[idx] as usize])
    }

    /// Returns the tick at which this component was last mutated for the entity.
    pub fn changed_tick(&self, entity_index: u32) -> Option<u64> {
        let idx = entity_index as usize;
        if idx >= self.sparse.len() || self.sparse[idx] == u32::MAX {
            return None;
        }
        Some(self.changed_ticks[self.sparse[idx] as usize])
    }

    /// Returns the monotonic change cursor for the last mutation on the entity.
    pub fn changed_cursor(&self, entity_index: u32) -> Option<u64> {
        let idx = entity_index as usize;
        if idx >= self.sparse.len() || self.sparse[idx] == u32::MAX {
            return None;
        }
        Some(self.changed_cursors[self.sparse[idx] as usize])
    }

    /// Marks all components in this set as changed at the given tick.
    /// Used by query_mut / iter_mut which grant blanket mutable access.
    pub fn mark_all_changed(&mut self, tick: u64, cursor: u64) {
        for (changed_tick, changed_cursor) in self
            .changed_ticks
            .iter_mut()
            .zip(self.changed_cursors.iter_mut())
        {
            *changed_tick = tick;
            *changed_cursor = cursor;
        }
    }

    /// Iterate entities whose component was changed after `since_tick`.
    pub fn iter_changed(&self, since_tick: u64) -> impl Iterator<Item = (u32, &T)> + '_ {
        self.dense
            .iter()
            .copied()
            .zip(self.data.iter().zip(self.changed_ticks.iter()))
            .filter(move |(_, (_, ct))| **ct > since_tick)
            .map(|(entity_idx, (data, _))| (entity_idx, data))
    }

    /// Iterate entities whose component was added after `since_tick`.
    pub fn iter_added(&self, since_tick: u64) -> impl Iterator<Item = (u32, &T)> + '_ {
        self.dense
            .iter()
            .copied()
            .zip(self.data.iter().zip(self.added_ticks.iter()))
            .filter(move |(_, (_, at))| **at > since_tick)
            .map(|(entity_idx, (data, _))| (entity_idx, data))
    }

    /// Returns `true` if this entity has the component.
    #[allow(dead_code)]
    pub fn contains(&self, entity_index: u32) -> bool {
        let idx = entity_index as usize;
        idx < self.sparse.len() && self.sparse[idx] != u32::MAX
    }

    /// Returns the number of entities that have this component.
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.dense.len()
    }

    /// Returns an iterator over (entity_index, &T) pairs.
    pub fn iter(&self) -> impl Iterator<Item = (u32, &T)> {
        self.dense
            .iter()
            .zip(self.data.iter())
            .map(|(&entity_idx, data)| (entity_idx, data))
    }

    /// Returns an iterator over (entity_index, &mut T) pairs.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (u32, &mut T)> {
        self.dense
            .iter()
            .zip(self.data.iter_mut())
            .map(|(&entity_idx, data)| (entity_idx, data))
    }

    /// Returns an iterator over entity indices present in the set.
    pub fn entity_indices(&self) -> impl Iterator<Item = u32> + '_ {
        self.dense.iter().copied()
    }
}

// =============================================================================
// AnyComponentStore — type erasure at the registry level
// =============================================================================

/// Trait object interface for component stores in the registry.
///
/// This allows `ComponentStorage` to hold heterogeneous `TypedSparseSet<T>`
/// values in a single `HashMap` while still supporting operations like
/// `remove_all` that don't need the concrete type.
pub(crate) trait AnyComponentStore: Any {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn remove_entry(&mut self, entity_index: u32) -> bool;
}

impl<T: 'static> AnyComponentStore for TypedSparseSet<T> {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn remove_entry(&mut self, entity_index: u32) -> bool {
        self.remove(entity_index)
    }
}

// =============================================================================
// ComponentStorage — typed registry keyed by TypeId
// =============================================================================

/// Registry of all component sparse sets, keyed by `TypeId`.
///
/// Stores `Box<dyn AnyComponentStore>` internally but provides typed access
/// via `typed_set` / `typed_set_mut` methods. The downcast from the trait
/// object happens once per query call (at the storage level), not once per
/// entity — a major improvement over the previous `Box<dyn Any>` per-component
/// design.
pub(crate) struct ComponentStorage {
    sets: HashMap<TypeId, Box<dyn AnyComponentStore>>,
}

impl ComponentStorage {
    pub fn new() -> Self {
        Self {
            sets: HashMap::new(),
        }
    }

    /// Get the typed sparse set for a component type (read-only).
    ///
    /// Returns `None` if no entities have this component type.
    pub fn typed_set<T: Component>(&self) -> Option<&TypedSparseSet<T>> {
        self.sets
            .get(&TypeId::of::<T>())
            .and_then(|s| s.as_any().downcast_ref::<TypedSparseSet<T>>())
    }

    /// Get or create the typed sparse set for a component type (mutable).
    pub fn typed_set_mut<T: Component>(&mut self) -> &mut TypedSparseSet<T> {
        self.sets
            .entry(TypeId::of::<T>())
            .or_insert_with(|| Box::new(TypedSparseSet::<T>::new()))
            .as_any_mut()
            .downcast_mut::<TypedSparseSet<T>>()
            .expect("TypeId mismatch in component storage")
    }

    /// Get the typed sparse set for a component type if it already exists.
    pub fn typed_set_existing_mut<T: Component>(&mut self) -> Option<&mut TypedSparseSet<T>> {
        self.sets
            .get_mut(&TypeId::of::<T>())
            .and_then(|s| s.as_any_mut().downcast_mut::<TypedSparseSet<T>>())
    }

    /// Remove all components for a given entity index.
    pub fn remove_all(&mut self, entity_index: u32) {
        for set in self.sets.values_mut() {
            set.remove_entry(entity_index);
        }
    }

    /// Get two mutable typed sparse sets at once.
    ///
    /// Panics if `A` and `B` are the same type.
    pub fn typed_sets_two_mut<A: Component, B: Component>(
        &mut self,
    ) -> (
        Option<&mut TypedSparseSet<A>>,
        Option<&mut TypedSparseSet<B>>,
    ) {
        assert_ne!(
            TypeId::of::<A>(),
            TypeId::of::<B>(),
            "cannot borrow the same sparse set mutably twice"
        );

        let ptr = &mut self.sets as *mut HashMap<TypeId, Box<dyn AnyComponentStore>>;
        // SAFETY: We asserted A != B, so we're borrowing two distinct entries.
        unsafe {
            let set_a = (*ptr)
                .get_mut(&TypeId::of::<A>())
                .and_then(|s| s.as_any_mut().downcast_mut::<TypedSparseSet<A>>());
            let set_b = (*ptr)
                .get_mut(&TypeId::of::<B>())
                .and_then(|s| s.as_any_mut().downcast_mut::<TypedSparseSet<B>>());
            (set_a, set_b)
        }
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_sparse_set_insert_get() {
        let mut set = TypedSparseSet::new();
        set.insert(5, 42_i32, 1, 1);
        assert_eq!(*set.get(5).unwrap(), 42);
    }

    #[test]
    fn typed_sparse_set_overwrite() {
        let mut set = TypedSparseSet::new();
        set.insert(0, 1_i32, 1, 1);
        set.insert(0, 2_i32, 1, 2);
        assert_eq!(*set.get(0).unwrap(), 2);
        assert_eq!(set.len(), 1);
    }

    #[test]
    fn typed_sparse_set_remove() {
        let mut set = TypedSparseSet::new();
        set.insert(0, 10_i32, 1, 1);
        set.insert(1, 20_i32, 1, 2);
        set.insert(2, 30_i32, 1, 3);
        assert!(set.remove(1));
        assert!(!set.contains(1));
        assert_eq!(set.len(), 2);

        // Remaining elements are still accessible.
        assert_eq!(*set.get(0).unwrap(), 10);
        assert_eq!(*set.get(2).unwrap(), 30);
    }

    #[test]
    fn typed_sparse_set_remove_nonexistent_returns_false() {
        let mut set = TypedSparseSet::<i32>::new();
        assert!(!set.remove(99));
    }

    #[test]
    fn typed_sparse_set_iteration() {
        let mut set = TypedSparseSet::new();
        set.insert(3, 30_i32, 1, 1);
        set.insert(7, 70_i32, 1, 2);

        let items: Vec<_> = set.iter().map(|(idx, &val)| (idx, val)).collect();
        assert_eq!(items.len(), 2);
        assert!(items.contains(&(3, 30)));
        assert!(items.contains(&(7, 70)));
    }
}
