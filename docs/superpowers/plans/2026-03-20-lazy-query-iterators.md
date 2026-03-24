# Lazy Query Iterators Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace Vec-returning query methods with lazy iterator structs that borrow directly from TypedSparseSet, eliminating heap allocation on every query call.

**Architecture:** New `query.rs` module defines iterator structs (`QueryIter`, `QueryIterMut`, `Query2Iter`, `Query2MutIter`, `Query3Iter`, `Query3MutIter`) that hold references to the sparse set's dense/data slices and the entity allocator. World methods return these iterators instead of `Vec`. Single-component iterators are fully safe; multi-component mutable iterators use the same `*mut TypedSparseSet` pattern already established in `query2_mut`.

**Tech Stack:** Rust 2024, no new dependencies.

---

## File Structure

| File | Action | Responsibility |
|------|--------|---------------|
| `crates/engine/src/query.rs` | **Create** | All iterator structs + Iterator impls |
| `crates/engine/src/component.rs` | Modify (lines 29-136) | Add `dense_data()` and `dense_data_mut()` slice accessors to `TypedSparseSet`; add `typed_sets_three_mut` to `ComponentStorage` |
| `crates/engine/src/world.rs` | Modify (lines 126-184) | Change query/query_mut/query2/query2_mut return types; add query3/query3_mut |
| `crates/engine/src/lib.rs` | Modify (line 9-28) | Add `pub mod query;` and re-export iterator types |
| `crates/engine/tests/query_iter.rs` | **Create** | Integration tests for all lazy iterators |
| `crates/engine-three-sync/src/extract.rs` | Modify (lines 23-27) | Remove `.iter()` on query result, drop `*e` deref |
| `crates/engine-three-sync/src/snapshot.rs` | Modify (lines 57-66) | Same pattern fix |
| `crates/engine/src/engine.rs` | Modify (tests only) | Remove `.into_iter()` on query results |
| `crates/engine/src/schedule.rs` | Modify (tests only) | Remove `.into_iter()` on query results |
| `crates/engine/src/game_loop.rs` | Modify (tests only) | Remove `.into_iter()` on query results |
| `docs/guide/ecs.md` | Modify | Update query examples |
| `docs/guide/plugins.md` | Verify | `for` loop on `query_mut` — works unchanged (IntoIterator) |
| `CHANGELOG.md` | Modify | Add entry |

---

## Caller Migration Cheat Sheet

When `query()` returns an iterator instead of Vec, callers change as follows:

| Old Pattern | New Pattern | Why |
|-------------|-------------|-----|
| `world.query::<T>().iter().map(...)` | `world.query::<T>().map(...)` | Result IS the iterator |
| `world.query::<T>().into_iter().map(...)` | `world.query::<T>().map(...)` | Same |
| `for (e, t) in world.query::<T>()` | No change | `for` calls IntoIterator, Iterator impls it |
| `let v = world.query::<T>(); v.len(); v[0]` | `let v: Vec<_> = world.query::<T>().collect(); v.len(); v[0]` | Vec-specific ops need collect |
| `(*e, t.position, ...)` | `(e, t.position, ...)` | Iterator yields `Entity` not `&Entity` |

---

### Task 1: Slice Accessors on TypedSparseSet

**Files:**
- Modify: `crates/engine/src/component.rs:29-136`

- [ ] **Step 1: Write test for dense_data accessor**

Add to the existing `mod tests` in `component.rs`:

```rust
#[test]
fn typed_sparse_set_dense_data_slices() {
    let mut set = TypedSparseSet::new();
    set.insert(3, 30_i32);
    set.insert(7, 70_i32);

    let (dense, data) = set.dense_data();
    assert_eq!(dense.len(), 2);
    assert_eq!(data.len(), 2);
    // Dense contains entity indices, data contains values (parallel).
    assert!(dense.contains(&3));
    assert!(dense.contains(&7));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p galeon-engine typed_sparse_set_dense_data_slices`
Expected: FAIL — `dense_data` method not found.

- [ ] **Step 3: Implement dense_data and dense_data_mut**

Add to `impl<T> TypedSparseSet<T>` block (after `iter_mut`, before closing `}`):

```rust
/// Returns immutable slices of (entity_indices, component_data).
///
/// Used by lazy query iterators to avoid re-borrowing the whole set.
pub(crate) fn dense_data(&self) -> (&[u32], &[T]) {
    (&self.dense, &self.data)
}

/// Returns the dense slice immutably and the data slice mutably.
///
/// Needed by `QueryIterMut` to iterate entity indices while yielding
/// `&mut T` references without reborrowing `&mut self`.
pub(crate) fn dense_data_mut(&mut self) -> (&[u32], &mut [T]) {
    (&self.dense, &mut self.data)
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p galeon-engine typed_sparse_set_dense_data_slices`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/engine/src/component.rs
git commit -m "feat(#11): add dense_data slice accessors to TypedSparseSet"
```

---

### Task 2: QueryIter — Immutable Single-Component Iterator

**Files:**
- Create: `crates/engine/src/query.rs`
- Modify: `crates/engine/src/world.rs:123-133`
- Modify: `crates/engine/src/lib.rs`

- [ ] **Step 1: Write failing integration test**

Create `crates/engine/tests/query_iter.rs`:

```rust
// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

use galeon_engine::{Component, World};

#[derive(Component, Debug, Clone, PartialEq)]
struct Pos {
    x: f32,
    y: f32,
}

#[derive(Component, Debug, Clone, PartialEq)]
struct Vel {
    dx: f32,
    dy: f32,
}

#[test]
fn query_iter_yields_matching_entities() {
    let mut world = World::new();
    world.spawn((Pos { x: 1.0, y: 0.0 },));
    world.spawn((Pos { x: 2.0, y: 0.0 }, Vel { dx: 0.0, dy: 0.0 }));
    world.spawn((Vel { dx: 3.0, dy: 0.0 },)); // no Pos

    // query() returns a lazy iterator — no Vec allocation.
    let positions: Vec<f32> = world.query::<Pos>().map(|(_, p)| p.x).collect();
    assert_eq!(positions.len(), 2);
    assert!(positions.contains(&1.0));
    assert!(positions.contains(&2.0));
}

#[test]
fn query_iter_empty_world() {
    let world = World::new();
    let results: Vec<_> = world.query::<Pos>().collect();
    assert!(results.is_empty());
}

#[test]
fn query_iter_entity_is_copy_not_ref() {
    let mut world = World::new();
    let e = world.spawn((Pos { x: 5.0, y: 0.0 },));

    // Entity from iterator is owned (Copy), not a reference.
    let (entity, pos) = world.query::<Pos>().next().unwrap();
    assert_eq!(entity, e);
    assert_eq!(pos.x, 5.0);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p galeon-engine --test query_iter query_iter_yields`
Expected: FAIL — either compilation error (query returns Vec not iterator) or test assertion failure.

- [ ] **Step 3: Create query.rs with QueryIter**

Create `crates/engine/src/query.rs`:

```rust
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
```

- [ ] **Step 4: Add `pub mod query;` to lib.rs**

Add after `pub mod world;` in `crates/engine/src/lib.rs`:

```rust
pub mod query;
```

And add re-export:

```rust
pub use query::QueryIter;
```

- [ ] **Step 5: Change World::query to return QueryIter**

Replace `World::query` method in `crates/engine/src/world.rs` (lines 123-133):

```rust
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
```

Add import at top of world.rs:

```rust
use crate::query::QueryIter;
```

- [ ] **Step 6: Fix callers in world.rs tests**

In `query_iterates_matching_entities` (line 255):
```rust
// Old: world.query::<Pos>().iter().map(|(_, p)| p.x).collect()
// New:
let positions: Vec<f32> = world.query::<Pos>().map(|(_, p)| p.x).collect();
```

In `query_mut_allows_modification` (line 271):
```rust
// Old: world.query::<Pos>().iter().map(|(_, p)| p.x).collect()
// New:
let xs: Vec<f32> = world.query::<Pos>().map(|(_, p)| p.x).collect();
```

- [ ] **Step 7: Fix callers in engine.rs, schedule.rs, game_loop.rs tests**

Remove `.into_iter()` from all `world.query::<T>().into_iter()` calls:

`schedule.rs` lines 109, 130, 151:
```rust
// Old: .query::<Counter>().into_iter().map(...)
// New: .query::<Counter>().map(...)
```

`game_loop.rs` lines 113, 138:
```rust
// Old: .query::<TickCounter>().into_iter().map(...)
// New: .query::<TickCounter>().map(...)
```

`engine.rs` lines 275, 308:
```rust
// Old: .query::<Counter>().into_iter().map(...)
// New: .query::<Counter>().map(...)
```

- [ ] **Step 8: Fix callers in engine-three-sync**

`extract.rs` lines 23-27:
```rust
// Old: world.query::<Transform>().iter().map(|(e, t)| (*e, t.position, ...))
// New:
let renderables: Vec<Renderable> = world
    .query::<Transform>()
    .map(|(e, t)| (e, t.position, t.rotation, t.scale))
    .collect();
```

`snapshot.rs` lines 57-66:
```rust
// Old: world.query::<Transform>().iter().map(|(e, t)| RawTransform { entity: *e, ... })
// New:
let renderables: Vec<RawTransform> = world
    .query::<Transform>()
    .map(|(e, t)| RawTransform {
        entity: e,
        position: t.position,
        rotation: t.rotation,
        scale: t.scale,
    })
    .collect();
```

- [ ] **Step 9: Run all tests**

Run: `cargo test --workspace`
Expected: ALL PASS

- [ ] **Step 10: Run clippy + fmt**

Run: `cargo clippy --workspace -- -D warnings && cargo fmt --check`
Expected: Clean

- [ ] **Step 11: Commit**

```bash
git add crates/engine/src/query.rs crates/engine/src/lib.rs crates/engine/src/world.rs \
       crates/engine/src/engine.rs crates/engine/src/schedule.rs crates/engine/src/game_loop.rs \
       crates/engine-three-sync/src/extract.rs crates/engine-three-sync/src/snapshot.rs \
       crates/engine/tests/query_iter.rs
git commit -m "feat(#11): add QueryIter — lazy immutable single-component queries"
```

---

### Task 3: QueryIterMut — Mutable Single-Component Iterator

**Files:**
- Modify: `crates/engine/src/query.rs`
- Modify: `crates/engine/src/world.rs:135-145`
- Modify: `crates/engine/tests/query_iter.rs`

- [ ] **Step 1: Write failing test**

Add to `crates/engine/tests/query_iter.rs`:

```rust
#[test]
fn query_iter_mut_allows_modification() {
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
fn query_iter_mut_empty_world() {
    let mut world = World::new();
    let results: Vec<_> = world.query_mut::<Pos>().collect();
    assert!(results.is_empty());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p galeon-engine --test query_iter query_iter_mut`
Expected: FAIL — `query_mut` still returns Vec, test may compile but we're checking the new iterator behavior.

- [ ] **Step 3: Add QueryIterMut to query.rs**

Append to `crates/engine/src/query.rs`:

```rust
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
```

- [ ] **Step 4: Add re-export to lib.rs**

```rust
pub use query::{QueryIter, QueryIterMut};
```

- [ ] **Step 5: Change World::query_mut to return QueryIterMut**

Replace in `crates/engine/src/world.rs` (lines 135-145):

```rust
/// Query all entities that have component T (mutable).
///
/// Returns a lazy iterator of `(Entity, &mut T)` pairs — no heap allocation.
pub fn query_mut<T: Component>(&mut self) -> QueryIterMut<'_, T> {
    let entities = &self.entities;
    let set = self.components.typed_set_mut::<T>();
    let (dense, data) = set.dense_data_mut();
    QueryIterMut::new(entities, dense, data)
}
```

Add to imports at top of world.rs:

```rust
use crate::query::{QueryIter, QueryIterMut};
```

- [ ] **Step 6: Run all tests**

Run: `cargo test --workspace`
Expected: ALL PASS (existing `for (_, pos) in world.query_mut::<T>()` callers work unchanged since Iterator implements IntoIterator)

- [ ] **Step 7: Run clippy + fmt**

Run: `cargo clippy --workspace -- -D warnings && cargo fmt --check`
Expected: Clean

- [ ] **Step 8: Commit**

```bash
git add crates/engine/src/query.rs crates/engine/src/lib.rs crates/engine/src/world.rs \
       crates/engine/tests/query_iter.rs
git commit -m "feat(#11): add QueryIterMut — lazy mutable single-component queries"
```

---

### Task 4: Query2Iter — Immutable Two-Component Iterator

**Files:**
- Modify: `crates/engine/src/query.rs`
- Modify: `crates/engine/src/world.rs:147-164`
- Modify: `crates/engine/tests/query_iter.rs`

- [ ] **Step 1: Write failing test**

Add to `crates/engine/tests/query_iter.rs`:

```rust
#[test]
fn query2_iter_yields_entities_with_both() {
    let mut world = World::new();
    world.spawn((Pos { x: 1.0, y: 0.0 },));
    world.spawn((Pos { x: 2.0, y: 0.0 }, Vel { dx: 5.0, dy: 0.0 }));
    world.spawn((Vel { dx: 3.0, dy: 0.0 },));

    let results: Vec<_> = world.query2::<Pos, Vel>().collect();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].1.x, 2.0);
    assert_eq!(results[0].2.dx, 5.0);
}

#[test]
fn query2_iter_empty_when_no_overlap() {
    let mut world = World::new();
    world.spawn((Pos { x: 1.0, y: 0.0 },));
    world.spawn((Vel { dx: 2.0, dy: 0.0 },));

    let results: Vec<_> = world.query2::<Pos, Vel>().collect();
    assert!(results.is_empty());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p galeon-engine --test query_iter query2_iter`
Expected: FAIL

- [ ] **Step 3: Add Query2Iter to query.rs**

Append to `crates/engine/src/query.rs`:

Add `use crate::component::TypedSparseSet;` to the **top** of `query.rs` alongside existing imports (not inline — `cargo fmt` requires imports at the top).

```rust
use crate::component::{Component, TypedSparseSet};
use crate::entity::{Entity, EntityAllocator};
```

Then append the `Query2Iter` struct:

```rust
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
            if let Some(b) = set_b.get(idx) {
                if let Some(entity) = self.entities.entity_at(idx) {
                    return Some((entity, a, b));
                }
            }
        }
        None
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.dense_a.len() - self.pos;
        (0, Some(remaining))
    }
}
```

- [ ] **Step 4: Add re-export to lib.rs**

```rust
pub use query::{Query2Iter, QueryIter, QueryIterMut};
```

- [ ] **Step 5: Change World::query2 to return Query2Iter**

Replace in `crates/engine/src/world.rs` (lines 147-164):

```rust
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
```

- [ ] **Step 6: Fix world.rs test that uses Vec indexing**

In `query2_returns_entities_with_both_components` (line 307):
```rust
// Old:
let results = world.query2::<Pos, Vel>();
assert_eq!(results.len(), 1);
assert_eq!(results[0].1.x, 2.0);
assert_eq!(results[0].2.x, 5.0);

// New:
let results: Vec<_> = world.query2::<Pos, Vel>().collect();
assert_eq!(results.len(), 1);
assert_eq!(results[0].1.x, 2.0);
assert_eq!(results[0].2.x, 5.0);
```

Add import in world.rs:
```rust
use crate::query::{Query2Iter, QueryIter, QueryIterMut};
```

- [ ] **Step 7: Run all tests**

Run: `cargo test --workspace`
Expected: ALL PASS

- [ ] **Step 8: Run clippy + fmt**

Run: `cargo clippy --workspace -- -D warnings && cargo fmt --check`
Expected: Clean

- [ ] **Step 9: Commit**

```bash
git add crates/engine/src/query.rs crates/engine/src/lib.rs crates/engine/src/world.rs \
       crates/engine/tests/query_iter.rs
git commit -m "feat(#11): add Query2Iter — lazy immutable two-component queries"
```

---

### Task 5: Query2MutIter — Mutable Two-Component Iterator

**Files:**
- Modify: `crates/engine/src/query.rs`
- Modify: `crates/engine/src/world.rs:166-184`
- Modify: `crates/engine/tests/query_iter.rs`

- [ ] **Step 1: Write failing test**

Add to `crates/engine/tests/query_iter.rs`:

```rust
#[test]
fn query2_mut_iter_mutates_both() {
    let mut world = World::new();
    let e = world.spawn((Pos { x: 1.0, y: 1.0 }, Vel { dx: 10.0, dy: 10.0 }));

    for (_, pos, vel) in world.query2_mut::<Pos, Vel>() {
        pos.x += 100.0;
        vel.dy += 200.0;
    }

    assert_eq!(world.get::<Pos>(e).unwrap().x, 101.0);
    assert_eq!(world.get::<Vel>(e).unwrap().dy, 210.0);
}

#[test]
fn query2_mut_iter_skips_missing() {
    let mut world = World::new();
    world.spawn((Pos { x: 5.0, y: 0.0 },));
    let e = world.spawn((Pos { x: 7.0, y: 0.0 }, Vel { dx: 9.0, dy: 0.0 }));
    world.spawn((Vel { dx: 11.0, dy: 0.0 },));

    let results: Vec<_> = world.query2_mut::<Pos, Vel>().collect();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].0, e);
}

#[test]
#[should_panic(expected = "cannot borrow the same sparse set mutably twice")]
fn query2_mut_iter_same_type_panics() {
    let mut world = World::new();
    world.spawn((Pos { x: 0.0, y: 0.0 },));
    let _ = world.query2_mut::<Pos, Pos>().collect::<Vec<_>>();
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p galeon-engine --test query_iter query2_mut`
Expected: FAIL

- [ ] **Step 3: Add Query2MutIter to query.rs**

Append to `crates/engine/src/query.rs`:

```rust
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
    /// Creates a new mutable two-component iterator.
    ///
    /// `sa` and `sb` must be distinct sparse sets (different TypeIds).
    /// Caller is responsible for ensuring this — `World::query2_mut`
    /// delegates to `typed_sets_two_mut` which panics if A == B.
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
```

- [ ] **Step 4: Add re-export to lib.rs**

```rust
pub use query::{Query2Iter, Query2MutIter, QueryIter, QueryIterMut};
```

- [ ] **Step 5: Change World::query2_mut to return Query2MutIter**

Replace in `crates/engine/src/world.rs` (lines 166-184):

```rust
/// Query all entities with two components (both mutable).
///
/// Returns a lazy iterator — no heap allocation. Panics if A == B.
pub fn query2_mut<A: Component, B: Component>(&mut self) -> Query2MutIter<'_, A, B> {
    let entities = &self.entities;
    let (set_a, set_b) = self.components.typed_sets_two_mut::<A, B>();
    let (Some(sa), Some(sb)) = (set_a, set_b) else {
        return Query2MutIter::empty(entities);
    };
    Query2MutIter::new(entities, sa, sb)
}
```

Add to imports:
```rust
use crate::query::{Query2Iter, Query2MutIter, QueryIter, QueryIterMut};
```

- [ ] **Step 6: Fix world.rs tests that use Vec indexing on query2_mut results**

In `query2_mut_skips_entities_missing_one_component` (line 340):
```rust
// Old:
let results = world.query2_mut::<Pos, Vel>();
assert_eq!(results.len(), 1);
let (entity, pos, vel) = &results[0];
// New:
let results: Vec<_> = world.query2_mut::<Pos, Vel>().collect();
assert_eq!(results.len(), 1);
let (entity, pos, vel) = &results[0];
```

- [ ] **Step 7: Run all tests**

Run: `cargo test --workspace`
Expected: ALL PASS

- [ ] **Step 8: Run clippy + fmt**

Run: `cargo clippy --workspace -- -D warnings && cargo fmt --check`
Expected: Clean

- [ ] **Step 9: Commit**

```bash
git add crates/engine/src/query.rs crates/engine/src/lib.rs crates/engine/src/world.rs \
       crates/engine/tests/query_iter.rs
git commit -m "feat(#11): add Query2MutIter — lazy mutable two-component queries"
```

---

### Task 6: Query3 — Three-Component Queries (New API)

**Files:**
- Modify: `crates/engine/src/component.rs` (add `typed_sets_three_mut`)
- Modify: `crates/engine/src/query.rs` (add `Query3Iter`, `Query3MutIter`)
- Modify: `crates/engine/src/world.rs` (add `query3`, `query3_mut`)
- Modify: `crates/engine/src/lib.rs`
- Modify: `crates/engine/tests/query_iter.rs`

- [ ] **Step 1: Write failing test for query3**

Add to `crates/engine/tests/query_iter.rs`:

```rust
#[derive(Component, Debug, Clone, PartialEq)]
struct Health(i32);

#[test]
fn query3_iter_yields_entities_with_all_three() {
    let mut world = World::new();
    world.spawn((Pos { x: 1.0, y: 0.0 }, Vel { dx: 2.0, dy: 0.0 }, Health(100)));
    world.spawn((Pos { x: 3.0, y: 0.0 }, Vel { dx: 4.0, dy: 0.0 })); // no Health
    world.spawn((Health(50),)); // no Pos or Vel

    let results: Vec<_> = world.query3::<Pos, Vel, Health>().collect();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].1.x, 1.0);
    assert_eq!(results[0].2.dx, 2.0);
    assert_eq!(results[0].3 .0, 100);
}

#[test]
fn query3_mut_iter_mutates_all_three() {
    let mut world = World::new();
    let e = world.spawn((Pos { x: 1.0, y: 0.0 }, Vel { dx: 2.0, dy: 0.0 }, Health(100)));

    for (_, pos, vel, hp) in world.query3_mut::<Pos, Vel, Health>() {
        pos.x += 10.0;
        vel.dx += 20.0;
        hp.0 -= 50;
    }

    assert_eq!(world.get::<Pos>(e).unwrap().x, 11.0);
    assert_eq!(world.get::<Vel>(e).unwrap().dx, 22.0);
    assert_eq!(world.get::<Health>(e).unwrap().0, 50);
}

#[test]
#[should_panic(expected = "cannot borrow the same sparse set")]
fn query3_mut_duplicate_type_panics() {
    let mut world = World::new();
    world.spawn((Pos { x: 0.0, y: 0.0 },));
    let _ = world.query3_mut::<Pos, Vel, Pos>().collect::<Vec<_>>();
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p galeon-engine --test query_iter query3`
Expected: FAIL — `query3` method doesn't exist.

- [ ] **Step 3: Add typed_sets_three_mut to ComponentStorage**

Add to `impl ComponentStorage` in `crates/engine/src/component.rs` (after `typed_sets_two_mut`):

```rust
/// Get three mutable typed sparse sets at once.
///
/// Panics if any two of A, B, C are the same type.
pub fn typed_sets_three_mut<A: Component, B: Component, C: Component>(
    &mut self,
) -> (
    Option<&mut TypedSparseSet<A>>,
    Option<&mut TypedSparseSet<B>>,
    Option<&mut TypedSparseSet<C>>,
) {
    assert_ne!(
        TypeId::of::<A>(),
        TypeId::of::<B>(),
        "cannot borrow the same sparse set mutably twice (A == B)"
    );
    assert_ne!(
        TypeId::of::<A>(),
        TypeId::of::<C>(),
        "cannot borrow the same sparse set mutably twice (A == C)"
    );
    assert_ne!(
        TypeId::of::<B>(),
        TypeId::of::<C>(),
        "cannot borrow the same sparse set mutably twice (B == C)"
    );

    let ptr = &mut self.sets as *mut HashMap<TypeId, Box<dyn AnyComponentStore>>;
    // SAFETY: We asserted A, B, C are all distinct, so we borrow three
    // separate entries from the map.
    unsafe {
        let set_a = (*ptr)
            .get_mut(&TypeId::of::<A>())
            .and_then(|s| s.as_any_mut().downcast_mut::<TypedSparseSet<A>>());
        let set_b = (*ptr)
            .get_mut(&TypeId::of::<B>())
            .and_then(|s| s.as_any_mut().downcast_mut::<TypedSparseSet<B>>());
        let set_c = (*ptr)
            .get_mut(&TypeId::of::<C>())
            .and_then(|s| s.as_any_mut().downcast_mut::<TypedSparseSet<C>>());
        (set_a, set_b, set_c)
    }
}
```

- [ ] **Step 4: Add Query3Iter and Query3MutIter to query.rs**

Append to `crates/engine/src/query.rs`:

```rust
/// Lazy iterator for immutable three-component queries.
pub struct Query3Iter<'w, A: Component, B: Component, C: Component> {
    entities: &'w EntityAllocator,
    dense_a: &'w [u32],
    data_a: &'w [A],
    set_b: Option<&'w TypedSparseSet<B>>,
    set_c: Option<&'w TypedSparseSet<C>>,
    pos: usize,
}

impl<'w, A: Component, B: Component, C: Component> Query3Iter<'w, A, B, C> {
    pub(crate) fn new(
        entities: &'w EntityAllocator,
        dense_a: &'w [u32],
        data_a: &'w [A],
        set_b: &'w TypedSparseSet<B>,
        set_c: &'w TypedSparseSet<C>,
    ) -> Self {
        Self {
            entities,
            dense_a,
            data_a,
            set_b: Some(set_b),
            set_c: Some(set_c),
            pos: 0,
        }
    }

    pub(crate) fn empty(entities: &'w EntityAllocator) -> Self {
        Self {
            entities,
            dense_a: &[],
            data_a: &[],
            set_b: None,
            set_c: None,
            pos: 0,
        }
    }
}

impl<'w, A: Component, B: Component, C: Component> Iterator for Query3Iter<'w, A, B, C> {
    type Item = (Entity, &'w A, &'w B, &'w C);

    fn next(&mut self) -> Option<Self::Item> {
        let set_b = self.set_b?;
        let set_c = self.set_c?;
        while self.pos < self.dense_a.len() {
            let idx = self.dense_a[self.pos];
            let a = &self.data_a[self.pos];
            self.pos += 1;
            if let (Some(b), Some(c)) = (set_b.get(idx), set_c.get(idx)) {
                if let Some(entity) = self.entities.entity_at(idx) {
                    return Some((entity, a, b, c));
                }
            }
        }
        None
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.dense_a.len() - self.pos;
        (0, Some(remaining))
    }
}

/// Lazy iterator for mutable three-component queries.
///
/// Same safety model as `Query2MutIter` — all three sets are distinct
/// (enforced by `typed_sets_three_mut`'s TypeId assertions).
pub struct Query3MutIter<'w, A: Component, B: Component, C: Component> {
    entities: &'w EntityAllocator,
    dense_a: &'w [u32],
    data_a: *mut A,
    len_a: usize,
    set_b: *mut TypedSparseSet<B>,
    set_c: *mut TypedSparseSet<C>,
    pos: usize,
    _marker: std::marker::PhantomData<&'w mut (A, B, C)>,
}

impl<'w, A: Component, B: Component, C: Component> Query3MutIter<'w, A, B, C> {
    pub(crate) fn new(
        entities: &'w EntityAllocator,
        sa: &'w mut TypedSparseSet<A>,
        sb: &'w mut TypedSparseSet<B>,
        sc: &'w mut TypedSparseSet<C>,
    ) -> Self {
        let (dense_a, data_a_slice) = sa.dense_data_mut();
        let len_a = data_a_slice.len();
        let data_a = data_a_slice.as_mut_ptr();
        Self {
            entities,
            dense_a,
            data_a,
            len_a,
            set_b: sb as *mut TypedSparseSet<B>,
            set_c: sc as *mut TypedSparseSet<C>,
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
            set_c: std::ptr::null_mut(),
            pos: 0,
            _marker: std::marker::PhantomData,
        }
    }
}

impl<'w, A: Component, B: Component, C: Component> Iterator for Query3MutIter<'w, A, B, C> {
    type Item = (Entity, &'w mut A, &'w mut B, &'w mut C);

    fn next(&mut self) -> Option<Self::Item> {
        while self.pos < self.len_a {
            let idx = self.dense_a[self.pos];
            let pos = self.pos;
            self.pos += 1;
            // SAFETY: Sets A, B, C are distinct (TypeId assertion in
            // typed_sets_three_mut). Each position yielded exactly once.
            unsafe {
                let a = &mut *self.data_a.add(pos);
                let b = (*self.set_b).get_mut(idx);
                let c = (*self.set_c).get_mut(idx);
                if let (Some(b), Some(c)) = (b, c) {
                    if let Some(entity) = self.entities.entity_at(idx) {
                        return Some((entity, a, b, c));
                    }
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
```

- [ ] **Step 5: Add query3 and query3_mut to World**

Add to `crates/engine/src/world.rs` (after `query2_mut`):

```rust
/// Query all entities with three components (all immutable).
///
/// Returns a lazy iterator — iterates set A, probes sets B and C.
pub fn query3<A: Component, B: Component, C: Component>(
    &self,
) -> Query3Iter<'_, A, B, C> {
    let (Some(set_a), Some(set_b), Some(set_c)) = (
        self.components.typed_set::<A>(),
        self.components.typed_set::<B>(),
        self.components.typed_set::<C>(),
    ) else {
        return Query3Iter::empty(&self.entities);
    };
    let (dense_a, data_a) = set_a.dense_data();
    Query3Iter::new(&self.entities, dense_a, data_a, set_b, set_c)
}

/// Query all entities with three components (all mutable).
///
/// Returns a lazy iterator. Panics if any two of A, B, C are the same type.
pub fn query3_mut<A: Component, B: Component, C: Component>(
    &mut self,
) -> Query3MutIter<'_, A, B, C> {
    let entities = &self.entities;
    let (set_a, set_b, set_c) = self.components.typed_sets_three_mut::<A, B, C>();
    let (Some(sa), Some(sb), Some(sc)) = (set_a, set_b, set_c) else {
        return Query3MutIter::empty(entities);
    };
    Query3MutIter::new(entities, sa, sb, sc)
}
```

Update imports at top of world.rs to include the new types:
```rust
use crate::query::{
    Query2Iter, Query2MutIter, Query3Iter, Query3MutIter, QueryIter, QueryIterMut,
};
```

- [ ] **Step 6: Add re-exports to lib.rs**

```rust
pub use query::{
    Query2Iter, Query2MutIter, Query3Iter, Query3MutIter, QueryIter, QueryIterMut,
};
```

- [ ] **Step 7: Run all tests**

Run: `cargo test --workspace`
Expected: ALL PASS

- [ ] **Step 8: Run clippy + fmt + WASM check**

Run: `cargo clippy --workspace -- -D warnings && cargo fmt --check`
Run: `cargo check --target wasm32-unknown-unknown -p galeon-engine-three-sync`
Expected: Clean

- [ ] **Step 9: Commit**

```bash
git add crates/engine/src/component.rs crates/engine/src/query.rs \
       crates/engine/src/lib.rs crates/engine/src/world.rs \
       crates/engine/tests/query_iter.rs
git commit -m "feat(#11): add query3/query3_mut — lazy three-component queries"
```

---

### Task 7: Documentation and Changelog

**Files:**
- Modify: `docs/guide/ecs.md`
- Modify: `CHANGELOG.md`

- [ ] **Step 1: Update ecs.md query section**

The guide examples use `for (entity, pos) in world.query::<Position>()` which still works (Iterator implements IntoIterator). Update the prose to mention lazy iterators and add a note about collecting when Vec-specific operations are needed.

Key changes:
- Remove any `.iter()` calls on query results in examples
- Add a note: "Queries return lazy iterators — call `.collect::<Vec<_>>()` if you need `len()` or indexing."
- Add `query3` and `query3_mut` examples

- [ ] **Step 2: Update CHANGELOG.md**

Add entry under Unreleased:

```markdown
### Changed
- **Queries return lazy iterators instead of `Vec`** — `query()`, `query_mut()`, `query2()`, `query2_mut()` now return zero-allocation iterator structs that borrow directly from the sparse set (#11)

### Added
- `query3()` and `query3_mut()` — three-component lazy queries (#11)
- `QueryIter`, `QueryIterMut`, `Query2Iter`, `Query2MutIter`, `Query3Iter`, `Query3MutIter` iterator types (#11)
```

- [ ] **Step 3: Final verification**

Run: `cargo test --workspace && cargo clippy --workspace -- -D warnings && cargo fmt --check`
Run: `cargo check --target wasm32-unknown-unknown -p galeon-engine-three-sync`
Expected: ALL PASS, clean

- [ ] **Step 4: Commit**

```bash
git add docs/guide/ecs.md CHANGELOG.md
git commit -m "docs(#11): document lazy query iterators and update changelog"
```

---

## Compilation Checkpoint

After Task 2 (QueryIter), run `cargo test --workspace` to verify no regressions before proceeding. This is the highest-risk task because it changes the return type of the most widely called method.

## Risk Notes

1. **QueryIterMut uses unsafe** — the raw pointer pattern yields each position exactly once (monotonically increasing `pos`). This is the standard Rust pattern for mutable iterators (same as `std::slice::IterMut` internally).

2. **Query2MutIter/Query3MutIter use unsafe** — same `*mut TypedSparseSet` pattern as the existing `query2_mut` Vec implementation. Safety relies on TypeId distinctness enforced at construction time.

3. **Breaking API change** — callers using `.iter()` or `.into_iter()` on query results, or using Vec indexing, must be updated. All known callers are listed in the Caller Migration Cheat Sheet above.

4. **WASM compatibility** — no new dependencies, no platform-specific code. Verify with `cargo check --target wasm32-unknown-unknown -p galeon-engine-three-sync`.
