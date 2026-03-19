// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

use std::any::{Any, TypeId};
use std::collections::HashMap;

/// Marker trait for types that can be stored as ECS components.
///
/// Derive with `#[derive(Component)]` from `galeon_engine_macros`.
pub trait Component: 'static {}

#[allow(dead_code)]
/// Type-erased storage for a single component type, backed by a sparse set.
///
/// Sparse set gives O(1) insert/get/remove and dense iteration — ideal for
/// RTS entities that frequently gain/lose components.
pub(crate) struct SparseSet {
    /// Sparse array: entity index → dense index (or `u32::MAX` if absent).
    sparse: Vec<u32>,
    /// Dense array of entity indices that have this component.
    dense: Vec<u32>,
    /// Parallel to `dense`: the actual component data (type-erased).
    data: Vec<Box<dyn Any>>,
}

#[allow(dead_code)]
impl SparseSet {
    pub fn new() -> Self {
        Self {
            sparse: Vec::new(),
            dense: Vec::new(),
            data: Vec::new(),
        }
    }

    /// Insert a component for an entity. Overwrites if already present.
    pub fn insert(&mut self, entity_index: u32, value: Box<dyn Any>) {
        let idx = entity_index as usize;

        // Grow sparse array if needed.
        if idx >= self.sparse.len() {
            self.sparse.resize(idx + 1, u32::MAX);
        }

        if self.sparse[idx] != u32::MAX {
            // Overwrite existing.
            let dense_idx = self.sparse[idx] as usize;
            self.data[dense_idx] = value;
        } else {
            // New entry.
            let dense_idx = self.dense.len() as u32;
            self.sparse[idx] = dense_idx;
            self.dense.push(entity_index);
            self.data.push(value);
        }
    }

    /// Get a reference to the component for an entity.
    pub fn get(&self, entity_index: u32) -> Option<&dyn Any> {
        let idx = entity_index as usize;
        if idx < self.sparse.len() && self.sparse[idx] != u32::MAX {
            let dense_idx = self.sparse[idx] as usize;
            Some(&*self.data[dense_idx])
        } else {
            None
        }
    }

    /// Get a mutable reference to the component for an entity.
    pub fn get_mut(&mut self, entity_index: u32) -> Option<&mut dyn Any> {
        let idx = entity_index as usize;
        if idx < self.sparse.len() && self.sparse[idx] != u32::MAX {
            let dense_idx = self.sparse[idx] as usize;
            Some(&mut *self.data[dense_idx])
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

        // Swap-remove from dense + data arrays.
        self.dense.swap(dense_idx, last_dense);
        self.data.swap(dense_idx, last_dense);
        self.dense.pop();
        self.data.pop();

        // Update the sparse entry for the swapped element.
        if dense_idx < self.dense.len() {
            let swapped_entity = self.dense[dense_idx] as usize;
            self.sparse[swapped_entity] = dense_idx as u32;
        }

        self.sparse[idx] = u32::MAX;
        true
    }

    /// Returns `true` if this entity has the component.
    pub fn contains(&self, entity_index: u32) -> bool {
        let idx = entity_index as usize;
        idx < self.sparse.len() && self.sparse[idx] != u32::MAX
    }

    /// Returns the number of entities that have this component.
    pub fn len(&self) -> usize {
        self.dense.len()
    }

    /// Returns an iterator over (entity_index, &dyn Any) pairs.
    pub fn iter(&self) -> impl Iterator<Item = (u32, &dyn Any)> {
        self.dense
            .iter()
            .zip(self.data.iter())
            .map(|(&entity_idx, data)| (entity_idx, &**data))
    }

    /// Returns an iterator over (entity_index, &mut dyn Any) pairs.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (u32, &mut dyn Any)> {
        self.dense
            .iter()
            .zip(self.data.iter_mut())
            .map(|(&entity_idx, data)| (entity_idx, &mut **data))
    }
}

#[allow(dead_code)]
/// Registry of all component sparse sets, keyed by `TypeId`.
pub(crate) struct ComponentStorage {
    sets: HashMap<TypeId, SparseSet>,
}

#[allow(dead_code)]
impl ComponentStorage {
    pub fn new() -> Self {
        Self {
            sets: HashMap::new(),
        }
    }

    /// Get or create the sparse set for a component type.
    pub fn set_mut<T: Component>(&mut self) -> &mut SparseSet {
        self.sets
            .entry(TypeId::of::<T>())
            .or_insert_with(SparseSet::new)
    }

    /// Get the sparse set for a component type (read-only).
    pub fn set<T: Component>(&self) -> Option<&SparseSet> {
        self.sets.get(&TypeId::of::<T>())
    }

    /// Get the sparse set for a given TypeId (read-only).
    pub fn set_by_id(&self, type_id: TypeId) -> Option<&SparseSet> {
        self.sets.get(&type_id)
    }

    /// Get the sparse set for a given TypeId (mutable).
    pub fn set_by_id_mut(&mut self, type_id: TypeId) -> Option<&mut SparseSet> {
        self.sets.get_mut(&type_id)
    }

    /// Remove all components for a given entity index.
    pub fn remove_all(&mut self, entity_index: u32) {
        for set in self.sets.values_mut() {
            set.remove(entity_index);
        }
    }

    /// Get two mutable sparse sets at once (for queries needing mutable access to multiple components).
    /// Panics if `a == b`.
    pub fn sets_two_mut(
        &mut self,
        a: TypeId,
        b: TypeId,
    ) -> (Option<&mut SparseSet>, Option<&mut SparseSet>) {
        assert_ne!(a, b, "cannot borrow the same sparse set mutably twice");

        let ptr = &mut self.sets as *mut HashMap<TypeId, SparseSet>;
        // SAFETY: We asserted a != b, so we're borrowing two distinct entries.
        unsafe {
            let set_a = (*ptr).get_mut(&a);
            let set_b = (*ptr).get_mut(&b);
            (set_a, set_b)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sparse_set_insert_get() {
        let mut set = SparseSet::new();
        set.insert(5, Box::new(42_i32));
        let val = set.get(5).unwrap().downcast_ref::<i32>().unwrap();
        assert_eq!(*val, 42);
    }

    #[test]
    fn sparse_set_overwrite() {
        let mut set = SparseSet::new();
        set.insert(0, Box::new(1_i32));
        set.insert(0, Box::new(2_i32));
        let val = set.get(0).unwrap().downcast_ref::<i32>().unwrap();
        assert_eq!(*val, 2);
        assert_eq!(set.len(), 1);
    }

    #[test]
    fn sparse_set_remove() {
        let mut set = SparseSet::new();
        set.insert(0, Box::new(10_i32));
        set.insert(1, Box::new(20_i32));
        set.insert(2, Box::new(30_i32));
        assert!(set.remove(1));
        assert!(!set.contains(1));
        assert_eq!(set.len(), 2);

        // Remaining elements are still accessible.
        assert_eq!(*set.get(0).unwrap().downcast_ref::<i32>().unwrap(), 10);
        assert_eq!(*set.get(2).unwrap().downcast_ref::<i32>().unwrap(), 30);
    }

    #[test]
    fn sparse_set_remove_nonexistent_returns_false() {
        let mut set = SparseSet::new();
        assert!(!set.remove(99));
    }

    #[test]
    fn sparse_set_iteration() {
        let mut set = SparseSet::new();
        set.insert(3, Box::new(30_i32));
        set.insert(7, Box::new(70_i32));

        let items: Vec<_> = set
            .iter()
            .map(|(idx, val)| (idx, *val.downcast_ref::<i32>().unwrap()))
            .collect();
        assert_eq!(items.len(), 2);
        assert!(items.contains(&(3, 30)));
        assert!(items.contains(&(7, 70)));
    }
}
