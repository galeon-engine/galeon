// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

//! Lazy query iterators — zero-allocation ECS queries.
//!
//! These iterators borrow directly from `TypedSparseSet` slices, avoiding
//! the heap allocation of `Vec<(Entity, &T)>` on every query call.

use crate::component::Component;
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
