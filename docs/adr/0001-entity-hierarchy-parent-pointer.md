# ADR-0001: Entity Hierarchy via Parent Pointer

**Status:** Accepted
**Date:** 2026-04-05
**Issue:** [#135](https://github.com/galeon-engine/galeon/issues/135)

## Context

Every entity's Three.js object was added flat to `scene.root`. No parent-child
relationships existed, blocking hierarchical models, transform inheritance,
and grouped visibility.

## Decision

### Parent-pointer only (no `Children` inverse)

`ParentEntity(Entity)` is the single source of truth. The renderer builds the
tree from parent pointers. A `Children` component is deferred — it's an ECS
convenience for game logic queries, not needed for the render path.

**Trade-off:** Removing a parent requires scanning `parentOf` to find orphans
(O(n) over tracked entities). An inverse `childrenOf` map would make removal
O(children) but adds bookkeeping complexity. The scan is acceptable because
hierarchies are typically shallow and despawns infrequent.

### Depth-sorted extraction in Rust

Entities are sorted parent-before-child during extraction so the JS-side
scene graph can be built in a single forward pass. The sort happens in Rust
(not TS) because hierarchy depth is available from the ECS.

**Trade-off:** Sorting adds O(n·d) work per extraction where d is max depth.
For the expected shallow hierarchies (d < 10) this is negligible.

### Two-pass RendererCache application

Pass 1 creates/updates all Three.js objects (unchanged from before hierarchy).
Pass 2 reparents objects based on `parent_ids`. This keeps the existing
single-pass logic intact and adds hierarchy as a layered concern.

### SCENE_ROOT sentinel

`u32::MAX` in `parent_ids` means "child of scene root." This avoids Option
overhead in the flat array and is a value that can never collide with a real
entity index.

## Consequences

- Hierarchical models, transform inheritance, and grouped visibility now work.
- `FramePacket` grows by one `Vec<u32>` (4 bytes per entity per frame).
- Incremental extraction detects `ParentEntity` changes via `CHANGED_PARENT`
  flag, but only for entities whose `Transform` also changed (same limitation
  as existing `Visibility`/`MeshHandle`/`MaterialHandle` detection).
- Future: a `Children` component and hierarchy maintenance systems may be
  added for ECS-side queries without changing the render path.
