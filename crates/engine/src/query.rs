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
    pub(crate) fn new(
        entities: &'w EntityAllocator,
        dense: &'w [u32],
        data: &'w mut [T],
    ) -> Self {
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
