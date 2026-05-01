# Three.js Sync — Render Extraction Pipeline

Galeon renders via Three.js. Rust owns all game state; TypeScript only drives
the Three.js scene graph. Data flows one way: **ECS → extraction → WASM
boundary → TS renderer cache → Three.js**.

## Two Paths

| Path | Purpose | Format | Crate / Package |
|------|---------|--------|-----------------|
| **Hot path** | Per-frame rendering | Flat typed arrays (`FramePacket`) | `galeon-engine-three-sync` → `@galeon/three` (or `@galeon/r3f`) |
| **Tooling path** | Inspector, profiler, shell | JSON (`DebugSnapshot`) | `galeon-engine-three-sync` |

The hot path is optimised for throughput — struct-of-arrays, no allocation per
entity, no serde. The tooling path prioritises readability — named fields,
`Option` for missing components, pretty-printed JSON.

## Render-Facing Components

Defined in `galeon-engine::render`. Any entity with a `Transform` is
considered renderable by the extraction system.

```rust
use galeon_engine::{MaterialHandle, MeshHandle, ObjectType, ParentEntity, Transform, Visibility};

// Required — makes the entity renderable.
Transform { position: [f32; 3], rotation: [f32; 4], scale: [f32; 3] }

// Optional — defaults to visible if absent.
Visibility { visible: bool }

// Optional — renderer maps ID to a Three.js BufferGeometry. 0 = no mesh.
MeshHandle { id: u32 }

// Optional — renderer maps ID to a Three.js Material. 0 = no material.
MaterialHandle { id: u32 }

// Optional — makes this entity a child of the referenced entity.
// Absent = child of scene root. Enables transform inheritance.
ParentEntity(Entity)

// Optional — selects the Three.js object class. Absent = Mesh.
ObjectType::Mesh | PointLight | DirectionalLight | LineSegments | Group
```

## Hot Path: FramePacket

`FramePacket` is a struct-of-arrays. All arrays are parallel — index `i` in
every array refers to the same entity.

```
entity_ids:       [u32; N]
transforms:       [f32; N * 10]   // per entity: pos(3) + rot(4) + scale(3)
visibility:       [u8;  N]        // 1 = visible, 0 = hidden
mesh_handles:     [u32; N]
material_handles: [u32; N]
parent_ids:       [u32; N]        // parent entity index; u32::MAX = scene root
object_types:     [u8;  N]        // 0=Mesh, 1=PointLight, 2=DirectionalLight, 3=LineSegments, 4=Group
change_flags:     [u8;  N]        // empty for full extract; bitmasks for incremental
```

**Transform stride is 10 floats:**

```
offset + 0..3  → position  (x, y, z)
offset + 3..7  → rotation  (x, y, z, w)  quaternion
offset + 7..10 → scale     (x, y, z)
```

### Extraction Cycle

Each frame:

1. Rust simulation systems run (game logic, physics).
2. `extract_frame(&World)` queries all `Transform` entities, probes optional
   components, packs data into `FramePacket`.
3. `WasmEngine.extract_frame()` returns a `WasmFramePacket` to JS.
4. `RendererCache.applyFrame(packet)` reads the typed arrays and applies bulk
   updates to the Three.js scene graph.

```
┌─ Rust ──────────────────────────────┐
│  ECS tick (simulation systems)      │
│           ↓                         │
│  extract_frame(&World)              │
│           ↓                         │
│  FramePacket (flat typed arrays)    │
└─────────────────────────────────────┘
           ↓  WASM boundary
┌─ TypeScript ────────────────────────┐
│  WasmFramePacket (getter access)    │
│           ↓                         │
│  RendererCache.applyFrame(packet)   │
│           ↓                         │
│  Three.js scene graph               │
└─────────────────────────────────────┘
```

### Borrow-Split Pattern

The extraction function uses a two-pass pattern to work within Rust's borrow
rules:

1. **Pass 1** — Query `Transform`, copy data into owned `Vec`. This releases
   the `&World` borrow.
2. **Pass 2** — For each entity, call `world.get::<Visibility>()`,
   `world.get::<MeshHandle>()`, etc. These are individual immutable borrows
   that don't conflict.

### WASM Boundary

`WasmFramePacket` exposes getter properties via `wasm_bindgen`. Each getter
clones the backing `Vec`, which wasm-bindgen converts to a JS typed array
(`Float32Array`, `Uint32Array`, `Uint8Array`).

`change_flags` is a parallel `Uint8Array` of per-entity bitmasks for incremental
extraction (`extract_frame_incremental`); it is empty for full `extract_frame`
packets. `@galeon/three`'s `RendererCache` uses these flags to skip redundant
Three.js writes when present.

**MVP transport:** copied flat buffers. Future optimisation: direct typed array
views into WASM linear memory (zero-copy).

### Consumer-Owned Bootstrap

`WasmEngine::new()` intentionally creates an empty ECS world. The generic
bridge crate does not seed app-specific entities or plugins.

If an app needs a non-empty first `extract_frame()`, own that bootstrap in a
thin Rust wrapper crate and configure the underlying engine before exposing the
handle to JavaScript:

```rust
use galeon_engine::{Engine, MaterialHandle, MeshHandle, Plugin, Transform, Visibility};
use galeon_engine_three_sync::{WasmEngine, WasmFramePacket};
use wasm_bindgen::prelude::*;

struct DemoBootstrapPlugin;

impl Plugin for DemoBootstrapPlugin {
    fn build(&self, engine: &mut Engine) {
        engine.world_mut().spawn((
            Transform {
                position: [0.0, 0.0, 0.0],
                rotation: [0.0, 0.0, 0.0, 1.0],
                scale: [2.0, 0.25, 2.0],
            },
            Visibility { visible: true },
            MeshHandle { id: 1 },
            MaterialHandle { id: 1 },
        ));
    }
}

#[wasm_bindgen]
pub struct DemoWasmEngine {
    inner: WasmEngine,
}

#[wasm_bindgen]
impl DemoWasmEngine {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        let mut inner = WasmEngine::new();
        inner.engine_mut().add_plugin(DemoBootstrapPlugin);
        Self { inner }
    }

    pub fn tick(&mut self, elapsed: f64) -> u32 {
        self.inner.tick(elapsed)
    }

    pub fn extract_frame(&self) -> WasmFramePacket {
        self.inner.extract_frame()
    }

    /// Spawn a renderable entity from JS. Returns [index, generation].
    pub fn spawn_entity(
        &mut self,
        mesh_id: u32,
        material_id: u32,
        transform: &[f32],
        object_type: u8,
    ) -> Vec<u32> {
        self.inner.spawn_entity(mesh_id, material_id, transform, object_type)
    }

    /// Despawn a JS-spawned entity. Returns false for plugin entities or stale handles.
    pub fn despawn_entity(&mut self, index: u32, generation: u32) -> bool {
        self.inner.despawn_entity(index, generation)
    }
}
```

If the app already has an `Engine` builder path, wrap that engine directly with
`WasmEngine::from_engine(engine)`.

### Dynamic Entity Management

JS can spawn and despawn entities at runtime without modifying the Rust plugin:

```typescript
// Spawn — returns [index, generation]
const transform = new Float32Array([0, 1, 0, 0, 0, 0, 1, 1, 1, 1]);
const [index, gen] = engine.spawn_entity(meshId, materialId, transform, 0);

// Despawn — returns false for plugin entities or stale handles
engine.despawn_entity(index, gen);

// Bulk cleanup — removes all JS-spawned entities
engine.despawn_all_js_entities();

// Leak detection
console.log(`JS entities: ${engine.js_entity_count()}`);
```

A `JsSpawned` marker component tags entities created via `spawn_entity`. Plugin-spawned
entities cannot be despawned from JS — `despawn_entity` returns `false` for them.

## TS Renderer Cache

`RendererCache` in `@galeon/three` manages the Three.js side:

```typescript
import { RendererCache } from "@galeon/three";

const cache = new RendererCache(scene);

// Register asset mappings.
cache.registerGeometry(1, myBoxGeometry);
cache.registerMaterial(1, myStandardMaterial);

// Per frame:
const packet = engine.extract_frame();
cache.applyFrame(packet);
```

**Per-frame behaviour (two-pass):**

**Pass 1 — Create/Update objects:**

- New entity IDs → create the requested `THREE.Object3D` type, add to scene (full row applied).
- Existing IDs → when `change_flags` is present, update only transform, visibility,
  and mesh/material resolution for bits set in the flag; when absent or empty,
  behave as a full update (same end state as before).
- `ObjectType` changes recreate the managed Three.js object while preserving
  the entity slot and hierarchy attachment.
- Missing IDs in **full** packets (no `change_flags`) → remove from scene.
  Incremental packets only include changed entities, so missing IDs do
  **not** trigger removal — absence means unchanged, not despawned.
- Unknown mesh/material handles → placeholder (magenta wireframe box).
- **Custom channels** (`custom_channel_*`) → copied into `userData` for every entity
  in the packet on every frame. They are **not** gated by `change_flags` today
  (no per-channel change bitmask in the protocol). Full `extract_frame` always
  carries channel payloads; incremental Rust extraction currently omits custom
  channels, so this mainly matters for full packets. Skipping redundant channel
  writes when flags exist is a plausible future optimization.

**Pass 2 — Reparent (hierarchy):**

- For each entity with `CHANGED_PARENT` flag (or all entities in full frames),
  compare the `parent_ids` value against the cached parent assignment.
- If the parent changed: detach from old parent, attach to new parent object
  (or scene root if `SCENE_ROOT`).
- Entities arrive depth-sorted from Rust extraction (parents before children),
  so a forward pass correctly builds the hierarchy.
- When a parent entity is removed, its children are reparented to the scene
  root to prevent orphan objects from becoming invisible.

## Migrating from `@galeon/engine-ts`

Galeon `0.5.0` removes the `@galeon/engine-ts` compatibility re-export package
that PR #206 introduced as a one-minor transition surface. ADR 0002 explicitly
scoped that shim as temporary; this is the close of that window.

If you imported anything from `@galeon/engine-ts`, switch to the canonical
home of each symbol:

| Old import | New import |
|------------|------------|
| `import { RendererCache } from "@galeon/engine-ts"` | `import { RendererCache } from "@galeon/three"` |
| `import { GALEON_ENTITY_KEY } from "@galeon/engine-ts"` | `import { GALEON_ENTITY_KEY } from "@galeon/three"` |
| `import type { RendererEntityHandle } from "@galeon/engine-ts"` | `import type { RendererEntityHandle } from "@galeon/three"` |
| `import { CHANGED_TRANSFORM, CHANGED_VISIBILITY, CHANGED_MESH, CHANGED_MATERIAL, CHANGED_OBJECT_TYPE, CHANGED_PARENT } from "@galeon/engine-ts"` | same names from `@galeon/render-core` |
| `import { ObjectType, SCENE_ROOT, TRANSFORM_STRIDE, RENDER_CONTRACT_VERSION } from "@galeon/engine-ts"` | same names from `@galeon/render-core` |
| `import { FramePacketContractError, assertFramePacketContract, hasIncrementalChangeFlags } from "@galeon/engine-ts"` | same names from `@galeon/render-core` |
| `import type { FramePacketContractOptions, FramePacketView } from "@galeon/engine-ts"` | same names from `@galeon/render-core` |
| `import { RUNTIME_VERSION, runtimeVersion } from "@galeon/engine-ts"` | `import { RUNTIME_VERSION } from "@galeon/runtime"` (the wrapper added no value) |

`package.json` consumers should drop the `@galeon/engine-ts` dependency and
add `@galeon/render-core` and `@galeon/three` (or `@galeon/r3f` for React
Three Fiber hosts) instead. The CLI's `local-first` scaffold has been
updated to emit the new dependency set on freshly generated projects.

The package is no longer published from this repository. Existing
`@galeon/engine-ts@0.4.x` releases remain on npm at their published
versions and will continue to resolve, but no further versions will be
cut.

## Adapter Choice

The render packet is the framework-neutral boundary. It belongs to Galeon, is
owned by Rust extraction, and should remain consumable without React, R3F, or a
browser-specific app shell. Host adapters decide how to present that packet.

Use the imperative Three adapter when the host wants direct scene-graph control:

- Engine-style consumers that already own the Three.js render loop.
- Desktop/editor surfaces that need predictable lifecycle hooks around object
  creation, removal, and GPU resource ownership.
- Tests and examples that should compare directly against the raw packet
  contract.
- Integrations that want to opt into demand rendering by checking
  `frame_version` and `RendererCache.needsRender`.

Use the R3F adapter when the host is a React application:

- Web games or tools with React-owned routing, menus, HUDs, inspectors, and DOM
  overlays around a Three.js scene.
- App shells that already use R3F components for cameras, controls, lights, or
  environment setup.
- Integrations that want a provider/component surface for structural scene
  lifecycle while still consuming Galeon-owned entity state.

The R3F adapter must not make React part of the engine core. React may own setup,
asset registry wiring, and structural lifecycle. Hot entity transforms should
continue to flow through retained Three object refs and frame-loop mutation, not
through per-entity React state updates on every tick.

Recommended integration pattern:

```tsx
import { GaleonEntities, GaleonProvider } from "@galeon/r3f";

<GaleonProvider engine={engine} driveTicks>
  <GaleonEntities />
</GaleonProvider>
```

Lower-level hooks can expose the current frame or individual entity metadata for
UI and tooling, but they should be treated as coarse subscriptions. The hot path
for transforms, visibility, and parent changes remains the adapter's imperative
application of packet rows to stable `THREE.Object3D` instances.

Non-goals:

- Requiring React, R3F, or any other UI framework in `galeon-engine` or
  `galeon-engine-three-sync`.
- Replacing the imperative Three adapter for consumers that do not use React.
- Mirroring the full entity graph as a JSX tree when the packet is a flat,
  generation-checked table with `parent_ids`.
- Using React state as the per-frame transport for every renderable entity.

## Tooling Path: DebugSnapshot

`WasmEngine.debug_snapshot()` returns a JSON string for inspector/shell
tooling:

```json
{
  "engine_version": "0.1.0",
  "entity_count": 2,
  "entities": [
    {
      "id": 0,
      "generation": 0,
      "transform": {
        "position": [1.0, 2.0, 3.0],
        "rotation": [0.0, 0.0, 0.0, 1.0],
        "scale": [1.0, 1.0, 1.0]
      },
      "visible": true,
      "mesh_handle": 10,
      "material_handle": 20
    }
  ]
}
```

This path is separate from the hot render path. It uses `serde_json` and named
fields for readability. Use it for the ECS inspector panel, profiler overlays,
and debug queries — never for per-frame rendering.

## Open Questions

- **Zero-copy transport**: Direct `Float32Array` views into WASM linear memory
  would avoid the clone in `WasmFramePacket` getters.
- **Interpolation**: How much visual interpolation should live in the TS
  renderer cache vs Rust extraction output.
- **Native host path**: When running in Electrobun (desktop), the extraction
  tables feed a native GPU renderer instead of Three.js.
- **Custom channels + incremental flags**: If incremental packets ever carry
  custom channel data, a `CHANGED_CUSTOM` (or per-channel) signal could let
  `RendererCache` skip `userData` writes the way `change_flags` skips transform
  work today.
