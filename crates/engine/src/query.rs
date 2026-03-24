// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

//! Lazy query iterators — zero-allocation ECS queries.
//!
//! These iterators borrow directly from `TypedSparseSet` slices, avoiding
//! the heap allocation of `Vec<(Entity, &T)>` on every query call.

use crate::component::{Component, TypedSparseSet};
use crate::entity::{Entity, EntityAllocator};

/// Lazy iterator for immutable single-component queries.
///
/// Yields `(Entity, &T)` for each entity that has component `T`.
/// Borrows the sparse set's dense + data slices directly — no heap allocation.
pub struct QueryIter<'w, T: Component> {
    entities: &'w EntityAllocator,
    dense: &'w [u32],
    data: &'w [T],
    pos: usize,
}

impl<'w, T: Component> QueryIter<'w, T> {
    pub(crate) fn new(entities: &'w EntityAllocator, dense: &'w [u32], data: &'w [T]) -> Self {
        Self {
            entities,
            dense,
            data,
            pos: 0,
        }
    }
}

impl<'w, T: Component> Iterator for QueryIter<'w, T> {
    type Item = (Entity, &'w T);

    fn next(&mut self) -> Option<Self::Item> {
        while self.pos < self.dense.len() {
            let idx = self.dense[self.pos];
            let val = &self.data[self.pos];
            self.pos += 1;
            if let Some(entity) = self.entities.entity_at(idx) {
                return Some((entity, val));
            }
        }
        None
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.dense.len() - self.pos;
        (0, Some(remaining))
    }
}

/// Lazy iterator for mutable single-component queries.
///
/// Yields `(Entity, &mut T)` for each entity that has component `T`.
/// Uses the split-borrow pattern: `dense` is borrowed immutably (entity indices)
/// while `data` is borrowed mutably (component values).
pub struct QueryIterMut<'w, T: Component> {
    entities: &'w EntityAllocator,
    dense: &'w [u32],
    data: *mut T,
    len: usize,
    pos: usize,
    _marker: std::marker::PhantomData<&'w mut T>,
}

impl<'w, T: Component> QueryIterMut<'w, T> {
    pub(crate) fn new(entities: &'w EntityAllocator, dense: &'w [u32], data: &'w mut [T]) -> Self {
        let len = data.len();
        let data = data.as_mut_ptr();
        Self {
            entities,
            dense,
            data,
            len,
            pos: 0,
            _marker: std::marker::PhantomData,
        }
    }
}

impl<'w, T: Component> Iterator for QueryIterMut<'w, T> {
    type Item = (Entity, &'w mut T);

    fn next(&mut self) -> Option<Self::Item> {
        while self.pos < self.len {
            let idx = self.dense[self.pos];
            let pos = self.pos;
            self.pos += 1;
            if let Some(entity) = self.entities.entity_at(idx) {
                // SAFETY: Each position is yielded exactly once (pos is
                // monotonically increasing). The data pointer comes from a
                // valid &mut [T] slice, and pos < len is checked above.
                let val = unsafe { &mut *self.data.add(pos) };
                return Some((entity, val));
            }
        }
        None
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.len - self.pos;
        (0, Some(remaining))
    }
}

/// Lazy iterator for immutable two-component queries.
///
/// Iterates entities in set A and probes set B for each.
/// Yields `(Entity, &A, &B)` for entities that have both components.
pub struct Query2Iter<'w, A: Component, B: Component> {
    entities: &'w EntityAllocator,
    dense_a: &'w [u32],
    data_a: &'w [A],
    set_b: Option<&'w TypedSparseSet<B>>,
    pos: usize,
}

impl<'w, A: Component, B: Component> Query2Iter<'w, A, B> {
    pub(crate) fn new(
        entities: &'w EntityAllocator,
        dense_a: &'w [u32],
        data_a: &'w [A],
        set_b: &'w TypedSparseSet<B>,
    ) -> Self {
        Self {
            entities,
            dense_a,
            data_a,
            set_b: Some(set_b),
            pos: 0,
        }
    }

    pub(crate) fn empty(entities: &'w EntityAllocator) -> Self {
        Self {
            entities,
            dense_a: &[],
            data_a: &[],
            set_b: None,
            pos: 0,
        }
    }
}

impl<'w, A: Component, B: Component> Iterator for Query2Iter<'w, A, B> {
    type Item = (Entity, &'w A, &'w B);

    fn next(&mut self) -> Option<Self::Item> {
        let set_b = self.set_b?;
        while self.pos < self.dense_a.len() {
            let idx = self.dense_a[self.pos];
            let a = &self.data_a[self.pos];
            self.pos += 1;
            if let Some(b) = set_b.get(idx)
                && let Some(entity) = self.entities.entity_at(idx)
            {
                return Some((entity, a, b));
            }
        }
        None
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.dense_a.len() - self.pos;
        (0, Some(remaining))
    }
}

/// Lazy iterator for mutable two-component queries.
///
/// Iterates set A mutably and probes set B mutably for each entity.
/// Uses raw pointers for set B access — same safety model as the
/// previous Vec-returning `query2_mut`.
///
/// # Safety invariants
///
/// - Sets A and B are distinct types (enforced by `typed_sets_two_mut`'s
///   TypeId assertion at construction time).
/// - Each entity index maps to a unique dense slot in each set, so
///   `get_mut` calls on set B never alias with set A's data.
pub struct Query2MutIter<'w, A: Component, B: Component> {
    entities: &'w EntityAllocator,
    dense_a: &'w [u32],
    data_a: *mut A,
    len_a: usize,
    set_b: *mut TypedSparseSet<B>,
    pos: usize,
    _marker: std::marker::PhantomData<&'w mut (A, B)>,
}

impl<'w, A: Component, B: Component> Query2MutIter<'w, A, B> {
    pub(crate) fn new(
        entities: &'w EntityAllocator,
        sa: &'w mut TypedSparseSet<A>,
        sb: &'w mut TypedSparseSet<B>,
    ) -> Self {
        let (dense_a, data_a_slice) = sa.dense_data_mut();
        let len_a = data_a_slice.len();
        let data_a = data_a_slice.as_mut_ptr();
        let set_b = sb as *mut TypedSparseSet<B>;
        Self {
            entities,
            dense_a,
            data_a,
            len_a,
            set_b,
            pos: 0,
            _marker: std::marker::PhantomData,
        }
    }

    pub(crate) fn empty(entities: &'w EntityAllocator) -> Self {
        Self {
            entities,
            dense_a: &[],
            data_a: std::ptr::null_mut(),
            len_a: 0,
            set_b: std::ptr::null_mut(),
            pos: 0,
            _marker: std::marker::PhantomData,
        }
    }
}

impl<'w, A: Component, B: Component> Iterator for Query2MutIter<'w, A, B> {
    type Item = (Entity, &'w mut A, &'w mut B);

    fn next(&mut self) -> Option<Self::Item> {
        while self.pos < self.len_a {
            let idx = self.dense_a[self.pos];
            let pos = self.pos;
            self.pos += 1;
            // SAFETY: sa and sb are distinct typed sparse sets (enforced by
            // typed_sets_two_mut's TypeId assertion). Each position is yielded
            // exactly once (pos is monotonically increasing). data_a[pos] and
            // sb.get_mut(idx) access separate heap allocations.
            unsafe {
                let Some(b) = (*self.set_b).get_mut(idx) else {
                    continue;
                };
                let a = &mut *self.data_a.add(pos);
                if let Some(entity) = self.entities.entity_at(idx) {
                    return Some((entity, a, b));
                }
            }
        }
        None
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.len_a - self.pos;
        (0, Some(remaining))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::World;

    #[derive(crate::Component, Debug, Clone, PartialEq)]
    struct TestPos {
        x: f32,
        y: f32,
    }

    #[test]
    fn query_iter_basic_functionality() {
        let mut world = World::new();
        world.spawn((TestPos { x: 1.0, y: 0.0 },));
        world.spawn((TestPos { x: 2.0, y: 0.0 },));

        // Test that query() returns an iterator directly
        let positions: Vec<f32> = world.query::<TestPos>().map(|(_, p)| p.x).collect();
        assert_eq!(positions.len(), 2);
        assert!(positions.contains(&1.0));
        assert!(positions.contains(&2.0));
    }

    #[test]
    fn query_iter_empty_world() {
        let world = World::new();
        let results: Vec<_> = world.query::<TestPos>().collect();
        assert!(results.is_empty());
    }
}
