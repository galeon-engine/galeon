# Object3D Type Diversity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace hardcoded `THREE.Mesh` creation in RendererCache with a factory that creates the correct Three.js object type (Mesh, PointLight, DirectionalLight, LineSegments, Group) based on an `ObjectType` component extracted from the ECS.

**Architecture:** Add an `ObjectType` enum component to the Rust ECS. Extract it as a `u8` array in FramePacket (parallel to existing arrays). On the TS side, generalize RendererCache's `objects` map from `Map<number, THREE.Mesh>` to `Map<number, THREE.Object3D>` and use a factory function keyed by object type. Lights need no geometry/material; meshes and line segments do.

**Tech Stack:** Rust (galeon-engine crate), TypeScript (bun:test, Three.js), wasm-bindgen

---

## File Structure

### Rust — new/modified files

| Action | File | Responsibility |
|--------|------|---------------|
| Modify | `crates/engine/src/render.rs` | Add `ObjectType` enum component |
| Modify | `crates/engine/src/lib.rs` | Re-export `ObjectType` |
| Modify | `crates/engine-three-sync/src/frame_packet.rs` | Add `object_types: Vec<u8>` field, `CHANGED_OBJECT_TYPE` flag |
| Modify | `crates/engine-three-sync/src/extract.rs` | Extract `ObjectType` into FramePacket |
| Modify | `crates/engine-three-sync/src/snapshot.rs` | Include `object_type` in `EntitySnapshot` |
| Modify | `crates/engine-three-sync/src/lib.rs` | Add `object_types()` getter to `WasmFramePacket`, accept `object_type` in `spawn_entity` |

### TypeScript — new/modified files

| Action | File | Responsibility |
|--------|------|---------------|
| Modify | `packages/engine-ts/src/types.ts` | Add `object_types` to `FramePacketView`, add `CHANGED_OBJECT_TYPE` constant, add `ObjectType` enum |
| Modify | `packages/engine-ts/src/renderer-cache.ts` | Generalize to `Object3D`, factory function, skip geometry/material for lights/groups |
| Modify | `packages/engine-ts/src/index.ts` | Re-export `ObjectType` |
| Modify | `packages/engine-ts/tests/renderer-cache.test.ts` | Tests for new object types |

### Docs

| Action | File | Responsibility |
|--------|------|---------------|
| Modify | `CHANGELOG.md` | Document the feature |

---

## Task 1: Add `ObjectType` enum to Rust ECS

**Files:**
- Modify: `crates/engine/src/render.rs:62` (after `MaterialHandle`)
- Modify: `crates/engine/src/lib.rs:60` (re-export line)

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)]` module in `crates/engine/src/render.rs`:

```rust
#[test]
fn object_type_default_is_mesh() {
    assert_eq!(ObjectType::default(), ObjectType::Mesh);
}

#[test]
fn object_type_as_u8() {
    assert_eq!(ObjectType::Mesh as u8, 0);
    assert_eq!(ObjectType::PointLight as u8, 1);
    assert_eq!(ObjectType::DirectionalLight as u8, 2);
    assert_eq!(ObjectType::LineSegments as u8, 3);
    assert_eq!(ObjectType::Group as u8, 4);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p galeon-engine render::tests::object_type --no-default-features`
Expected: FAIL — `ObjectType` not defined.

- [ ] **Step 3: Write the implementation**

Add after `MaterialHandle` in `crates/engine/src/render.rs`:

```rust
/// What kind of Three.js object to create for this entity.
///
/// Extracted as a `u8` in the FramePacket. The TS renderer uses this
/// to pick the correct constructor (Mesh, PointLight, etc.).
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ObjectType {
    /// `THREE.Mesh` — the default for renderable entities.
    Mesh = 0,
    /// `THREE.PointLight` — omni-directional light source.
    PointLight = 1,
    /// `THREE.DirectionalLight` — sun-like parallel light.
    DirectionalLight = 2,
    /// `THREE.LineSegments` — debug line rendering.
    LineSegments = 3,
    /// `THREE.Group` — container for hierarchy.
    Group = 4,
}

impl Default for ObjectType {
    fn default() -> Self {
        Self::Mesh
    }
}
```

- [ ] **Step 4: Add re-export**

In `crates/engine/src/lib.rs`, change the render re-export line from:

```rust
pub use render::{MaterialHandle, MeshHandle, Transform, Visibility};
```

to:

```rust
pub use render::{MaterialHandle, MeshHandle, ObjectType, Transform, Visibility};
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p galeon-engine render::tests --no-default-features`
Expected: PASS — all render tests pass including new ones.

- [ ] **Step 6: Commit**

```bash
git add crates/engine/src/render.rs crates/engine/src/lib.rs
git commit -m "feat(#134): add ObjectType enum component to ECS

Five variants: Mesh (default), PointLight, DirectionalLight,
LineSegments, Group. Repr u8 for efficient FramePacket extraction.

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>"
```

---

## Task 2: Add `object_types` to FramePacket

**Files:**
- Modify: `crates/engine-three-sync/src/frame_packet.rs`

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)]` module in `crates/engine-three-sync/src/frame_packet.rs`:

```rust
#[test]
fn push_stores_object_type() {
    let mut p = FramePacket::new();
    p.push(
        1,
        0,
        &[0.0; 3],
        &[0.0, 0.0, 0.0, 1.0],
        &[1.0; 3],
        true,
        10,
        20,
        2, // DirectionalLight
    );
    assert_eq!(p.entity_count(), 1);
    assert_eq!(p.object_types[0], 2);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p galeon-engine-three-sync frame_packet::tests::push_stores_object_type`
Expected: FAIL — `push` doesn't accept `object_type` param.

- [ ] **Step 3: Implement changes to FramePacket**

In `crates/engine-three-sync/src/frame_packet.rs`:

a. Add the new change flag constant after `CHANGED_MATERIAL`:
```rust
pub const CHANGED_OBJECT_TYPE: u8 = 1 << 4;
```

b. Add `object_types` field to `FramePacket` struct (after `material_handles`):
```rust
pub object_types: Vec<u8>,
```

c. Initialize in `new()` and `with_capacity()`:
```rust
// In new():
object_types: Vec::new(),

// In with_capacity():
object_types: Vec::with_capacity(entity_count),
```

d. Add `object_type: u8` parameter to `push()` (after `material_id: u32`):
```rust
pub fn push(
    &mut self,
    entity_id: u32,
    entity_generation: u32,
    position: &[f32; 3],
    rotation: &[f32; 4],
    scale: &[f32; 3],
    visible: bool,
    mesh_id: u32,
    material_id: u32,
    object_type: u8,
) {
    self.entity_ids.push(entity_id);
    self.entity_generations.push(entity_generation);
    self.transforms.extend_from_slice(position);
    self.transforms.extend_from_slice(rotation);
    self.transforms.extend_from_slice(scale);
    self.visibility.push(visible as u8);
    self.mesh_handles.push(mesh_id);
    self.material_handles.push(material_id);
    self.object_types.push(object_type);
}
```

e. Update `push_incremental` to also accept `object_type: u8` (pass through to `push`):
```rust
pub fn push_incremental(
    &mut self,
    entity_id: u32,
    generation: u32,
    position: &[f32; 3],
    rotation: &[f32; 4],
    scale: &[f32; 3],
    visible: bool,
    mesh_id: u32,
    material_id: u32,
    object_type: u8,
    flags: u8,
) {
    self.push(
        entity_id, generation, position, rotation, scale,
        visible, mesh_id, material_id, object_type,
    );
    self.change_flags.push(flags);
}
```

- [ ] **Step 4: Fix existing tests that call `push`**

All existing `push()` calls in the test module need the new `object_type` parameter. Add `0` (Mesh) as the last arg to every existing `push()` call in the test module:

- `push_one_entity`: add `0,` after `20,`
- `push_multiple_entities`: add `0,` after `1,` and `0,` after `3,`

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p galeon-engine-three-sync frame_packet::tests`
Expected: PASS — all frame_packet tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/engine-three-sync/src/frame_packet.rs
git commit -m "feat(#134): add object_types array to FramePacket

Parallel u8 array carrying ObjectType discriminant per entity.
New CHANGED_OBJECT_TYPE flag (bit 4) for incremental extraction.

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>"
```

---

## Task 3: Extract `ObjectType` into FramePacket

**Files:**
- Modify: `crates/engine-three-sync/src/extract.rs`

- [ ] **Step 1: Write the failing test**

Add to `crates/engine-three-sync/src/extract.rs` test module:

```rust
use galeon_engine::ObjectType;

#[test]
fn extract_object_type_component() {
    let mut world = World::new();
    world.spawn((
        Transform::from_position(1.0, 0.0, 0.0),
        ObjectType::PointLight,
    ));
    world.spawn((
        Transform::from_position(2.0, 0.0, 0.0),
        ObjectType::Mesh,
    ));
    world.spawn((Transform::from_position(3.0, 0.0, 0.0),));

    let packet = extract_frame(&world);
    assert_eq!(packet.entity_count(), 3);

    // Find the PointLight entity (position.x == 1.0)
    let light_idx = packet.entity_ids.iter().enumerate()
        .find(|(i, _)| packet.transforms[*i * 10] == 1.0)
        .unwrap().0;
    assert_eq!(packet.object_types[light_idx], ObjectType::PointLight as u8);

    // Entity without ObjectType defaults to Mesh (0)
    let bare_idx = packet.entity_ids.iter().enumerate()
        .find(|(i, _)| packet.transforms[*i * 10] == 3.0)
        .unwrap().0;
    assert_eq!(packet.object_types[bare_idx], ObjectType::Mesh as u8);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p galeon-engine-three-sync extract::tests::extract_object_type_component`
Expected: FAIL — extraction doesn't query `ObjectType`.

- [ ] **Step 3: Update extraction functions**

In `crates/engine-three-sync/src/extract.rs`:

a. Add `ObjectType` to the import line:
```rust
use galeon_engine::render::{MaterialHandle, MeshHandle, Transform, Visibility};
use galeon_engine::{Entity, ObjectType, RenderChannelRegistry, World};
```

b. Update the query in `extract_frame` to include `Option<&ObjectType>`:
```rust
let query = world.query::<(
    &Transform,
    Option<&Visibility>,
    Option<&MeshHandle>,
    Option<&MaterialHandle>,
    Option<&ObjectType>,
)>();
```

c. Update the loop to unpack and pass `object_type`:
```rust
for (entity, (transform, vis, mesh, mat, obj_type)) in query {
    packet.push(
        entity.index(),
        entity.generation(),
        &transform.position,
        &transform.rotation,
        &transform.scale,
        vis.map(|v| v.visible).unwrap_or(true),
        mesh.map(|m| m.id).unwrap_or(0),
        mat.map(|m| m.id).unwrap_or(0),
        obj_type.map(|t| *t as u8).unwrap_or(0),
    );
    entities.push(entity);
}
```

d. Update the `Renderable` type alias to include `u8`:
```rust
type Renderable = (Entity, [f32; 3], [f32; 4], [f32; 3], u8);
```

e. Update `extract_frame_incremental` similarly — add `ObjectType` to the component lookup:
```rust
let renderables: Vec<Renderable> = world
    .query_changed::<Transform>(since_tick)
    .map(|(e, t)| {
        let obj_type = world.get::<ObjectType>(e).map(|o| *o as u8).unwrap_or(0);
        (e, t.position, t.rotation, t.scale, obj_type)
    })
    .collect();
```

f. Pass `object_type` through the incremental loop:
```rust
for (entity, position, rotation, scale, object_type) in &renderables {
    // ... existing flags logic ...

    // Add ObjectType change detection:
    if let Some(loc) = world.entity_location(*entity) {
        let arch = world.archetypes().get(loc.archetype_id);
        let row = loc.row as usize;
        // ... existing flag checks ...
        if arch
            .column::<ObjectType>()
            .is_some_and(|c| c.changed_tick(row) > since_tick)
        {
            flags |= CHANGED_OBJECT_TYPE;
        }
    }

    packet.push_incremental(
        entity.index(),
        entity.generation(),
        position,
        rotation,
        scale,
        visible,
        mesh_id,
        material_id,
        *object_type,
        flags,
    );
}
```

g. Add `CHANGED_OBJECT_TYPE` to the imports from `frame_packet`:
```rust
use crate::frame_packet::{
    CHANGED_MATERIAL, CHANGED_MESH, CHANGED_OBJECT_TYPE, CHANGED_TRANSFORM,
    CHANGED_VISIBILITY, ChannelData, FramePacket,
};
```

- [ ] **Step 4: Fix existing extraction tests**

All existing `push()` calls in the extraction test module are indirect (via `extract_frame`), so they don't need manual fixing — the extraction function now passes the object_type. Verify by running all tests.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p galeon-engine-three-sync extract::tests`
Expected: PASS — all extraction tests pass including the new one.

- [ ] **Step 6: Commit**

```bash
git add crates/engine-three-sync/src/extract.rs
git commit -m "feat(#134): extract ObjectType into FramePacket

Both full and incremental extraction now query Option<&ObjectType>
and pack it as u8 (default 0 = Mesh). Incremental path detects
ObjectType changes via CHANGED_OBJECT_TYPE flag.

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>"
```

---

## Task 4: Update snapshot and WASM bridge

**Files:**
- Modify: `crates/engine-three-sync/src/snapshot.rs`
- Modify: `crates/engine-three-sync/src/lib.rs`

- [ ] **Step 1: Write failing tests**

a. In `crates/engine-three-sync/src/snapshot.rs` test module, add:

```rust
use galeon_engine::ObjectType;

#[test]
fn snapshot_includes_object_type() {
    let mut world = World::new();
    world.spawn((
        Transform::from_position(1.0, 0.0, 0.0),
        ObjectType::PointLight,
    ));

    let snap = extract_debug_snapshot(&world);
    let e = &snap.entities[0];
    assert_eq!(e.object_type, Some("PointLight".to_string()));
}

#[test]
fn snapshot_object_type_none_when_absent() {
    let mut world = World::new();
    world.spawn((Transform::identity(),));

    let snap = extract_debug_snapshot(&world);
    assert_eq!(snap.entities[0].object_type, None);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p galeon-engine-three-sync snapshot::tests::snapshot_includes_object_type`
Expected: FAIL — `object_type` field not on `EntitySnapshot`.

- [ ] **Step 3: Update snapshot**

a. Add `ObjectType` to imports in `snapshot.rs`:
```rust
use galeon_engine::render::{MaterialHandle, MeshHandle, ObjectType, Transform, Visibility};
```

Note: `ObjectType` needs to be re-exported from `galeon_engine::render` — it already is via `lib.rs` re-export, but the snapshot imports from `galeon_engine::render` directly. Since we added it to `render.rs` and the `lib.rs` re-export, importing `ObjectType` from `galeon_engine` or from `galeon_engine::render` both work.

b. Add `object_type` field to `EntitySnapshot`:
```rust
pub struct EntitySnapshot {
    pub id: u32,
    pub generation: u32,
    pub transform: Option<TransformSnapshot>,
    pub visible: Option<bool>,
    pub mesh_handle: Option<u32>,
    pub material_handle: Option<u32>,
    pub object_type: Option<String>,
    pub custom_channels: HashMap<String, Vec<f32>>,
}
```

c. Update the query in `extract_debug_snapshot` to include `Option<&ObjectType>`:
```rust
let query = world.query::<(
    &Transform,
    Option<&Visibility>,
    Option<&MeshHandle>,
    Option<&MaterialHandle>,
    Option<&ObjectType>,
)>();
```

d. In the loop, add `obj_type` to the destructure and populate the new field:
```rust
for (entity, (transform, vis, mesh, mat, obj_type)) in query {
    // ... existing code ...
    entities.push(EntitySnapshot {
        id: entity.index(),
        generation: entity.generation(),
        transform: Some(TransformSnapshot { ... }),
        visible: vis.map(|v| v.visible),
        mesh_handle: mesh.map(|m| m.id),
        material_handle: mat.map(|m| m.id),
        object_type: obj_type.map(|t| format!("{:?}", t)),
        custom_channels,
    });
}
```

- [ ] **Step 4: Update WasmFramePacket and spawn_entity in lib.rs**

a. Add `CHANGED_OBJECT_TYPE` to the re-export in `crates/engine-three-sync/src/lib.rs`:
```rust
pub use frame_packet::{
    CHANGED_MATERIAL, CHANGED_MESH, CHANGED_OBJECT_TYPE, CHANGED_TRANSFORM,
    CHANGED_VISIBILITY, ChannelData, FramePacket, TRANSFORM_STRIDE,
};
```

b. Add `ObjectType` to the import from `galeon_engine`:
```rust
use galeon_engine::{Component, Engine, Entity, MaterialHandle, MeshHandle, ObjectType, Transform, Visibility};
```

c. Add `object_types` getter to `WasmFramePacket`:
```rust
/// Object type discriminants (one u8 per entity: 0=Mesh, 1=PointLight, 2=DirectionalLight, 3=LineSegments, 4=Group).
#[wasm_bindgen(getter)]
pub fn object_types(&self) -> Vec<u8> {
    self.inner.object_types.clone()
}
```

d. Update `spawn_entity` to accept `object_type: u8`:
```rust
pub fn spawn_entity(&mut self, mesh_id: u32, material_id: u32, transform: &[f32], object_type: u8) -> Vec<u32> {
    if transform.len() < TRANSFORM_STRIDE {
        return Vec::new();
    }
    let obj_type = match object_type {
        1 => ObjectType::PointLight,
        2 => ObjectType::DirectionalLight,
        3 => ObjectType::LineSegments,
        4 => ObjectType::Group,
        _ => ObjectType::Mesh,
    };
    let entity = self.engine.world_mut().spawn((
        Transform {
            position: [transform[0], transform[1], transform[2]],
            rotation: [transform[3], transform[4], transform[5], transform[6]],
            scale: [transform[7], transform[8], transform[9]],
        },
        Visibility { visible: true },
        MeshHandle { id: mesh_id },
        MaterialHandle { id: material_id },
        obj_type,
        JsSpawned,
    ));
    vec![entity.index(), entity.generation()]
}
```

- [ ] **Step 5: Fix existing integration tests**

The `spawn_entity` signature changed — update calls in `crates/engine-three-sync/tests/dynamic_spawn_despawn.rs` to add `0` as the new `object_type` parameter (after the transform slice argument).

Also update `crates/engine-three-sync/tests/consumer_owned_bootstrap.rs` if it calls `spawn_entity`.

- [ ] **Step 6: Run all tests**

Run: `cargo test -p galeon-engine-three-sync`
Expected: PASS — all snapshot, WASM bridge, and integration tests pass.

- [ ] **Step 7: Commit**

```bash
git add crates/engine-three-sync/
git commit -m "feat(#134): expose object_types through WASM bridge and snapshot

WasmFramePacket.object_types getter, spawn_entity accepts object_type
parameter, DebugSnapshot includes object_type as readable string.

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>"
```

---

## Task 5: Update TypeScript types and RendererCache

**Files:**
- Modify: `packages/engine-ts/src/types.ts`
- Modify: `packages/engine-ts/src/renderer-cache.ts`
- Modify: `packages/engine-ts/src/index.ts`

- [ ] **Step 1: Write the failing test**

Add to `packages/engine-ts/tests/renderer-cache.test.ts`:

```typescript
import { CHANGED_OBJECT_TYPE } from "../src/types.js";

describe("RendererCache Object3D type diversity", () => {
  test("creates THREE.Mesh for ObjectType 0 (default)", () => {
    const scene = new THREE.Scene();
    const cache = new RendererCache(scene);
    cache.registerGeometry(1, new THREE.BoxGeometry());
    cache.registerMaterial(1, new THREE.MeshBasicMaterial());

    cache.applyFrame(makePacket({
      entity_count: 1,
      entity_ids: new Uint32Array([1]),
      entity_generations: new Uint32Array([0]),
      mesh_handles: new Uint32Array([1]),
      material_handles: new Uint32Array([1]),
      object_types: new Uint8Array([0]),
    }));

    const obj = cache.getObject(1, 0)!;
    expect(obj).toBeInstanceOf(THREE.Mesh);
  });

  test("creates THREE.PointLight for ObjectType 1", () => {
    const scene = new THREE.Scene();
    const cache = new RendererCache(scene);

    cache.applyFrame(makePacket({
      entity_count: 1,
      entity_ids: new Uint32Array([1]),
      entity_generations: new Uint32Array([0]),
      object_types: new Uint8Array([1]),
    }));

    const obj = cache.getObject(1, 0)!;
    expect(obj).toBeInstanceOf(THREE.PointLight);
  });

  test("creates THREE.DirectionalLight for ObjectType 2", () => {
    const scene = new THREE.Scene();
    const cache = new RendererCache(scene);

    cache.applyFrame(makePacket({
      entity_count: 1,
      entity_ids: new Uint32Array([1]),
      entity_generations: new Uint32Array([0]),
      object_types: new Uint8Array([2]),
    }));

    const obj = cache.getObject(1, 0)!;
    expect(obj).toBeInstanceOf(THREE.DirectionalLight);
  });

  test("creates THREE.LineSegments for ObjectType 3", () => {
    const scene = new THREE.Scene();
    const cache = new RendererCache(scene);
    cache.registerGeometry(1, new THREE.BufferGeometry());
    cache.registerMaterial(1, new THREE.LineBasicMaterial());

    cache.applyFrame(makePacket({
      entity_count: 1,
      entity_ids: new Uint32Array([1]),
      entity_generations: new Uint32Array([0]),
      mesh_handles: new Uint32Array([1]),
      material_handles: new Uint32Array([1]),
      object_types: new Uint8Array([3]),
    }));

    const obj = cache.getObject(1, 0)!;
    expect(obj).toBeInstanceOf(THREE.LineSegments);
  });

  test("creates THREE.Group for ObjectType 4", () => {
    const scene = new THREE.Scene();
    const cache = new RendererCache(scene);

    cache.applyFrame(makePacket({
      entity_count: 1,
      entity_ids: new Uint32Array([1]),
      entity_generations: new Uint32Array([0]),
      object_types: new Uint8Array([4]),
    }));

    const obj = cache.getObject(1, 0)!;
    expect(obj).toBeInstanceOf(THREE.Group);
  });

  test("lights ignore geometry and material handles", () => {
    const scene = new THREE.Scene();
    const cache = new RendererCache(scene);
    const warnSpy = spyOn(console, "warn").mockImplementation(() => {});

    cache.applyFrame(makePacket({
      entity_count: 1,
      entity_ids: new Uint32Array([1]),
      entity_generations: new Uint32Array([0]),
      mesh_handles: new Uint32Array([99]),
      material_handles: new Uint32Array([88]),
      object_types: new Uint8Array([1]), // PointLight
    }));

    // Lights should NOT warn about missing mesh/material handles
    expect(warnSpy).toHaveBeenCalledTimes(0);
    warnSpy.mockRestore();
  });

  test("mixed object types in one frame", () => {
    const scene = new THREE.Scene();
    const cache = new RendererCache(scene);
    cache.registerGeometry(1, new THREE.BoxGeometry());
    cache.registerMaterial(1, new THREE.MeshBasicMaterial());

    cache.applyFrame(makePacket({
      entity_count: 3,
      entity_ids: new Uint32Array([1, 2, 3]),
      entity_generations: new Uint32Array([0, 0, 0]),
      mesh_handles: new Uint32Array([1, 0, 1]),
      material_handles: new Uint32Array([1, 0, 1]),
      object_types: new Uint8Array([0, 1, 3]), // Mesh, PointLight, LineSegments
    }));

    expect(cache.getObject(1, 0)).toBeInstanceOf(THREE.Mesh);
    expect(cache.getObject(2, 0)).toBeInstanceOf(THREE.PointLight);
    expect(cache.getObject(3, 0)).toBeInstanceOf(THREE.LineSegments);
    expect(cache.objectCount).toBe(3);
  });

  test("absent object_types array defaults to Mesh", () => {
    const scene = new THREE.Scene();
    const cache = new RendererCache(scene);
    cache.registerGeometry(1, new THREE.BoxGeometry());
    cache.registerMaterial(1, new THREE.MeshBasicMaterial());

    // Packet without object_types field — backward compatibility
    cache.applyFrame(makePacket({
      entity_count: 1,
      entity_ids: new Uint32Array([1]),
      entity_generations: new Uint32Array([0]),
      mesh_handles: new Uint32Array([1]),
      material_handles: new Uint32Array([1]),
    }));

    expect(cache.getObject(1, 0)).toBeInstanceOf(THREE.Mesh);
  });

  test("onEntityRemoved fires with Object3D for non-Mesh types", () => {
    const scene = new THREE.Scene();
    const cache = new RendererCache(scene);
    const removed: THREE.Object3D[] = [];
    cache.onEntityRemoved = (_id, _gen, obj) => { removed.push(obj); };

    cache.applyFrame(makePacket({
      entity_count: 1,
      entity_ids: new Uint32Array([1]),
      entity_generations: new Uint32Array([0]),
      object_types: new Uint8Array([1]), // PointLight
    }));

    cache.applyFrame(makePacket({ entity_count: 0 }));
    expect(removed.length).toBe(1);
    expect(removed[0]).toBeInstanceOf(THREE.PointLight);
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd packages/engine-ts && bun test`
Expected: FAIL — `CHANGED_OBJECT_TYPE` not exported, `object_types` not on `FramePacketView`, `getObject` returns `THREE.Mesh | undefined` not `THREE.Object3D | undefined`.

- [ ] **Step 3: Update types.ts**

In `packages/engine-ts/src/types.ts`:

a. Add the new change flag constant:
```typescript
/** Object type changed — matches Rust `CHANGED_OBJECT_TYPE`. */
export const CHANGED_OBJECT_TYPE = 1 << 4;
```

b. Add `ObjectType` enum:
```typescript
/** Object type discriminant — values match Rust `ObjectType` repr(u8). */
export const enum ObjectType {
  Mesh = 0,
  PointLight = 1,
  DirectionalLight = 2,
  LineSegments = 3,
  Group = 4,
}
```

c. Add `object_types` to `FramePacketView`:
```typescript
export interface FramePacketView {
  readonly entity_count: number;
  readonly entity_ids: Uint32Array;
  readonly entity_generations: Uint32Array;
  readonly transforms: Float32Array;
  readonly visibility: Uint8Array;
  readonly mesh_handles: Uint32Array;
  readonly material_handles: Uint32Array;
  /** Set for incremental extraction; omit or empty for full frames (all fields apply). */
  readonly change_flags?: Uint8Array;
  /** Object type per entity (0=Mesh, 1=PointLight, 2=DirectionalLight, 3=LineSegments, 4=Group). Omit for all-Mesh frames. */
  readonly object_types?: Uint8Array;
  readonly custom_channel_count: number;
  custom_channel_name_at(index: number): string;
  custom_channel_stride(name: string): number;
  custom_channel_data(name: string): Float32Array;
}
```

- [ ] **Step 4: Update renderer-cache.ts**

a. Update imports to include new constants and `ObjectType`:
```typescript
import {
  CHANGED_MATERIAL,
  CHANGED_MESH,
  CHANGED_TRANSFORM,
  CHANGED_VISIBILITY,
  type FramePacketView,
  ObjectType,
  TRANSFORM_STRIDE,
} from "./types.js";
```

b. Generalize the `objects` map type:
```typescript
private readonly objects = new Map<number, THREE.Object3D>();
```

c. Change `onEntityRemoved` callback signature:
```typescript
onEntityRemoved?: (entityId: number, generation: number, obj: THREE.Object3D) => void;
```

d. Change `getObject` return type:
```typescript
getObject(entityId: number, generation: number): THREE.Object3D | undefined {
```

e. Add factory method (before `removeEntity`):
```typescript
/**
 * Returns true if this object type uses geometry and material.
 * Lights and Groups don't need them — skip registry resolution and warnings.
 */
private static needsGeometryMaterial(objectType: number): boolean {
  return objectType === ObjectType.Mesh || objectType === ObjectType.LineSegments;
}

private createObject(
  objectType: number,
  geometry: THREE.BufferGeometry,
  material: THREE.Material,
): THREE.Object3D {
  switch (objectType) {
    case ObjectType.PointLight:
      return new THREE.PointLight();
    case ObjectType.DirectionalLight:
      return new THREE.DirectionalLight();
    case ObjectType.LineSegments:
      return new THREE.LineSegments(geometry, material);
    case ObjectType.Group:
      return new THREE.Group();
    default:
      return new THREE.Mesh(geometry, material);
  }
}
```

f. Update `applyFrame` entity creation path. Replace lines 132–147 with:
```typescript
if (!obj) {
  const meshHandle = mesh_handles[i]!;
  const matHandle = material_handles[i]!;
  const objectType = packet.object_types?.[i] ?? ObjectType.Mesh;
  const needsGeoMat = RendererCache.needsGeometryMaterial(objectType);
  const geometry = needsGeoMat
    ? (this.geometries.get(meshHandle) ?? this.placeholderGeometry)
    : this.placeholderGeometry;
  const material = needsGeoMat
    ? (this.materials.get(matHandle) ?? this.placeholderMaterial)
    : this.placeholderMaterial;
  obj = this.createObject(objectType, geometry, material);
  obj.matrixAutoUpdate = false;
  this.objects.set(entityId, obj);
  this.generations.set(entityId, generation);
  if (needsGeoMat) {
    this.resolvedGeometries.set(entityId, geometry);
    this.resolvedMaterials.set(entityId, material);
    this.warnMissingHandles(entityId, meshHandle, matHandle);
  }
  (obj.userData as Record<PropertyKey, unknown>)[GALEON_ENTITY_KEY] = { entityId, generation };
  this.scene.add(obj);
  this.applyTransform(obj, i, transforms);
  obj.visible = visibility[i]! === 1;
}
```

g. Update `removeEntity`, `applyTransform`, and `clear` signatures to use `THREE.Object3D`:
```typescript
private removeEntity(id: number, obj: THREE.Object3D): void {
```
```typescript
private applyTransform(obj: THREE.Object3D, i: number, transforms: Float32Array): void {
```

- [ ] **Step 5: Update index.ts re-exports**

In `packages/engine-ts/src/index.ts`, add `ObjectType` and `CHANGED_OBJECT_TYPE` to the types re-export:
```typescript
export {
  CHANGED_MATERIAL,
  CHANGED_MESH,
  CHANGED_OBJECT_TYPE,
  CHANGED_TRANSFORM,
  CHANGED_VISIBILITY,
  type FramePacketView,
  ObjectType,
  TRANSFORM_STRIDE,
} from "./types.js";
```

- [ ] **Step 6: Update makePacket helper in tests**

The `makePacket` helper in `renderer-cache.test.ts` needs to pass through `object_types` from overrides. No change needed — `...overrides` already handles it since `object_types` is optional on `FramePacketView`.

- [ ] **Step 7: Run tests**

Run: `cd packages/engine-ts && bun test`
Expected: PASS — all existing and new tests pass.

- [ ] **Step 8: Commit**

```bash
git add packages/engine-ts/
git commit -m "feat(#134): generalize RendererCache to Object3D types

objects map is now Map<number, THREE.Object3D>. Factory creates Mesh,
PointLight, DirectionalLight, LineSegments, or Group based on the
object_types array from FramePacket. Lights and Groups skip geometry/
material resolution and missing-handle warnings. Backward-compatible:
absent object_types defaults to all Mesh.

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>"
```

---

## Task 6: Cross-stack verification and CHANGELOG

**Files:**
- Run: full Rust + TS test suites
- Modify: `CHANGELOG.md`

- [ ] **Step 1: Run full Rust test suite**

Run: `cargo test --workspace`
Expected: PASS — all tests green.

- [ ] **Step 2: Run Rust clippy**

Run: `cargo clippy -- -D warnings`
Expected: PASS — no warnings.

- [ ] **Step 3: Run TS type-check**

Run: `cd packages/engine-ts && bun run check` (or `bun tsc --noEmit`)
Expected: PASS — no type errors.

- [ ] **Step 4: Run TS tests**

Run: `cd packages/engine-ts && bun test`
Expected: PASS — all tests green.

- [ ] **Step 5: Check WASM compilation**

Run: `cargo check --target wasm32-unknown-unknown -p galeon-engine-three-sync`
Expected: PASS — compiles for WASM target.

- [ ] **Step 6: Update CHANGELOG.md**

Add under `## [Unreleased]` → `### Added`:

```markdown
- **`ObjectType` component and Object3D type diversity in RendererCache** —
  Entities can now specify their Three.js representation via an `ObjectType`
  component (Mesh, PointLight, DirectionalLight, LineSegments, Group). The
  RendererCache factory creates the correct object type, skipping geometry/material
  resolution for types that don't need them. Backward-compatible: entities without
  `ObjectType` default to Mesh.
  ([#134](https://github.com/galeon-engine/galeon/issues/134))
```

- [ ] **Step 7: Commit**

```bash
git add CHANGELOG.md
git commit -m "docs(#134): add Object3D type diversity to CHANGELOG

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>"
```
