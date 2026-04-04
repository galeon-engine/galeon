# Three.js Sync — Render Extraction Pipeline

Galeon renders via Three.js. Rust owns all game state; TypeScript only drives
the Three.js scene graph. Data flows one way: **ECS → extraction → WASM
boundary → TS renderer cache → Three.js**.

## Two Paths

| Path | Purpose | Format | Crate / Package |
|------|---------|--------|-----------------|
| **Hot path** | Per-frame rendering | Flat typed arrays (`FramePacket`) | `galeon-engine-three-sync` → `@galeon/engine-ts` |
| **Tooling path** | Inspector, profiler, shell | JSON (`DebugSnapshot`) | `galeon-engine-three-sync` |

The hot path is optimised for throughput — struct-of-arrays, no allocation per
entity, no serde. The tooling path prioritises readability — named fields,
`Option` for missing components, pretty-printed JSON.

## Render-Facing Components

Defined in `galeon-engine::render`. Any entity with a `Transform` is
considered renderable by the extraction system.

```rust
use galeon_engine::{Transform, Visibility, MeshHandle, MaterialHandle};

// Required — makes the entity renderable.
Transform { position: [f32; 3], rotation: [f32; 4], scale: [f32; 3] }

// Optional — defaults to visible if absent.
Visibility { visible: bool }

// Optional — renderer maps ID to a Three.js BufferGeometry. 0 = no mesh.
MeshHandle { id: u32 }

// Optional — renderer maps ID to a Three.js Material. 0 = no material.
MaterialHandle { id: u32 }
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
}
```

If the app already has an `Engine` builder path, wrap that engine directly with
`WasmEngine::from_engine(engine)`.

## TS Renderer Cache

`RendererCache` in `@galeon/engine-ts` manages the Three.js side:

```typescript
import { RendererCache } from "@galeon/engine-ts";

const cache = new RendererCache(scene);

// Register asset mappings.
cache.registerGeometry(1, myBoxGeometry);
cache.registerMaterial(1, myStandardMaterial);

// Per frame:
const packet = engine.extract_frame();
cache.applyFrame(packet);
```

**Per-frame behaviour:**

- New entity IDs → create `THREE.Mesh`, add to scene.
- Existing IDs → update position/quaternion/scale/visibility.
- Missing IDs (were present last frame) → remove from scene.
- Unknown mesh/material handles → placeholder (magenta wireframe box).

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
