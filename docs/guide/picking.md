# Mouse Picking And Drag-Rectangle Selection

`@galeon/picking` wires the browser's mouse events to a Galeon scene: clicks
resolve to ECS entities and drag-rectangles return every entity inside the
rect. The selected entities live on the Rust side as a `Selection` resource so
game systems can react in their own systems.

## Data Flow

```
mouse events (canvas)
   │
   ▼
@galeon/picking ──── pick / pick-rect events ───▶ WasmEngine.applyPick / applyPickRect
                                                     │
                                                     ▼
                                              galeon_engine::Selection (resource)
                                                     │
                                                     ▼
                                              game systems read via Res<Selection>
```

The TypeScript side owns the raycaster, NDC math, and rect-to-frustum
projection. The Rust side owns the modifier semantics and the authoritative
selection state. The WASM bridge ferries typed events between them.

## TypeScript: `attachPicking`

```ts
import * as THREE from "three";
import { attachPicking, type PickingEvent } from "@galeon/picking";

const dispose = attachPicking(canvas, scene, camera, {
  onPick(event: PickingEvent) {
    if (event.kind === "pick") {
      // Single click. event.entity is { entityId, generation } | null.
      // event.point is the world-space hit point or null.
    } else {
      // Drag rectangle. event.entities is an array of { entityId, generation }.
    }
    // event.modifiers carries shift/ctrl/alt/meta from the originating MouseEvent.
  },
  dragThreshold: 4, // pixels of movement before a drag becomes a marquee
});

// On unmount or camera/scene swap:
dispose();
```

`attachPicking` walks ancestor chains, so a child sub-mesh of a `Group`
resolves to the entity stamped on the group. NDC math uses
`getBoundingClientRect()`, so the canvas does not need to be fullscreen.

`attachPicking` also accepts a `pickingBackend` option. Omitting it or passing
`"galeon"` uses Galeon's default backend: standalone objects still use
`THREE.Raycaster`, while `@galeon/three` instanced batches use their
`InstancedMesh2` BVH for click and marquee queries. Pass `"raycaster"` only when
debugging the raw Three.js path. Custom backends receive
`pickAt({ scene, camera, ndc })` and
`pickRect({ scene, camera, ndcStart, ndcEnd, filter })` requests and must return
the same `{ entityId, generation }` refs, so accelerated implementations can
swap in without changing Rust-side `Selection` semantics.

Instanced render batches also resolve to Galeon entity handles for click
picking and drag rectangles. `@galeon/three` owns `InstancedMesh2` batches from
`@three.ez/instanced-mesh`, computes a per-instance BVH, and stamps each batch
with an `instanceId -> { entityId, generation }` resolver. `@galeon/picking`
queries that BVH directly for instanced clicks and sub-frustum marquee
selection, then uses the resolver to emit the same entity refs as standalone
objects. This avoids the legacy `THREE.InstancedMesh.raycast` first-hit
limitation instead of adding a GPU-readback fallback.

## TypeScript: `attachMarqueeRenderer`

```ts
import { attachMarqueeRenderer } from "@galeon/picking";

const marquee = attachMarqueeRenderer(camera);

function onDrag(startNdc, endNdc) {
  marquee.update({ start: startNdc, end: endNdc });
}

function onDragEnd() {
  marquee.update(null);
}

// On unmount:
marquee.dispose();
```

`attachMarqueeRenderer` is a visual-only HUD primitive. It renders the current
drag rectangle as camera-attached Three.js line geometry using Normalised
Device Coordinates. It does not emit picking events or modify `Selection`;
pair it with `attachPicking` when a project wants both selection behavior and
the standard in-engine drag rectangle.

## TypeScript: `attachSelectionRings`

```ts
import { attachSelectionRings } from "@galeon/picking";

const rings = attachSelectionRings(scene, rendererCache);

function render() {
  rings.update(wasm.selectionEntities());
  renderer.render(scene, camera);
}

// On unmount:
rings.dispose();
```

`attachSelectionRings` renders simple `THREE.LineLoop` rings in world space
for selected entities resolved through a `RendererCache`-compatible
target. Standalone entities resolve through `getObject(entityId, generation)`;
instanced entities resolve through `getInstance(entityId, generation)` and draw
against the selected `THREE.InstancedMesh` slot. It intentionally uses
per-entity wire rings instead of a post-processing `OutlinePass`, so consumers
do not need to adopt `EffectComposer` just to show selection. Call
`update(selection)` after selection changes or once per render frame if
selected objects keep moving.

## React Three Fiber Bindings

```tsx
import { GaleonProvider, MarqueeRenderer, SelectionRings } from "@galeon/r3f";

<GaleonProvider engine={engine}>
  <MarqueeRenderer rect={dragRectNdc} />
  <SelectionRings selection={selectionEntities} />
</GaleonProvider>
```

`<MarqueeRenderer />` attaches the rectangle geometry to the active R3F camera.
`<SelectionRings />` reads the `RendererCache` from `GaleonProvider` and
refreshes ring transforms during the R3F frame loop.

## Picking Baseline

Run the standalone baseline harness with:

```bash
bun run --cwd packages/picking bench:baseline
```

The harness drives the public `attachPicking` event path against deterministic
standalone `THREE.Mesh` entities. Click picks use the current
`Raycaster.intersectObjects(scene.children, true)` path; marquee picks use the
current six-plane sub-frustum plus per-entity world-AABB path. Each operation
uses 25 warmup iterations and 125 measured samples.

Baseline captured on May 2, 2026 with Bun 1.3.8 on Windows x64:

| Entities | Operation | Median ms | P95 ms | Result size |
| ---: | --- | ---: | ---: | ---: |
| 100 | click | 0.017 | 0.028 | 1 |
| 100 | marquee | 0.038 | 0.055 | 100 |
| 1,000 | click | 0.063 | 0.077 | 1 |
| 1,000 | marquee | 0.141 | 0.332 | 1,000 |
| 10,000 | click | 0.544 | 1.018 | 1 |
| 10,000 | marquee | 1.649 | 3.017 | 10,000 |

Use the default raycaster backend for ordinary standalone scenes up to roughly
1,000 pickable entities. At 10,000 standalone entities, click remains fine for
discrete input, but marquee p95 is already around 3 ms in a headless
microbenchmark; treat continuous drag updates, hover picking, dense static
geometry, and per-instance marquee selection as the threshold for a BVH or GPU
backend. These numbers are a baseline for comparing backend work under #224,
not a browser performance guarantee.

Run the instanced BVH comparison with:

```bash
bun run --cwd packages/picking bench:instanced-bvh
```

That harness compares Galeon's BVH backend against a deliberately linear
per-instance backend on the same 10,000-cube `InstancedMesh2` scene. Baseline
captured on May 2, 2026 with Bun 1.3.8 on Windows x64:

| Backend | Operation | Median ms | P95 ms | Result size |
| --- | --- | ---: | ---: | ---: |
| linear | click | 1.186 | 1.960 | 1 |
| linear | marquee | 0.976 | 1.230 | 484 |
| bvh | click | 0.010 | 0.019 | 1 |
| bvh | marquee | 0.021 | 0.034 | 484 |

The BVH path is roughly 120x faster for the measured instanced click and 45x
faster for the measured instanced marquee. The result sizes match, so the
acceleration preserves selection semantics for the benchmarked scene.

## Rust: `Selection` resource

```rust
use galeon_engine::{Engine, PickModifiers, Res, Selection};

let mut engine = Engine::new();
engine.world_mut().insert_resource(Selection::new());

fn highlight_selected(selection: Res<Selection>) {
    for entity in &selection.entities {
        // …issue a movement order, draw a highlight ring, etc.
    }
}
```

The WASM bridge lazy-installs the `Selection` resource on first input event,
so explicit `insert_resource` is only needed for native examples or tools that
read it before any input fires.

## Modifier Semantics

| Modifier | Click | Marquee |
|----------|-------|---------|
| (none)   | Replace selection (or clear on miss) | Replace selection |
| Shift    | Toggle entity in/out (no-op on miss) | Add to selection |
| Ctrl     | Remove entity (no-op on miss)        | Remove from selection |
| Alt      | (treated as no-modifier on hit)      | Intersect (keep entities in both) |
| Multiple modifiers (Shift+Ctrl, Ctrl+Alt, …) | Replace on hit, no-op on miss | Replace |

These follow the StarCraft / OpenRA consensus catalogued in the discovery
notes for issue #214. Both `apply_pick` and `apply_pick_rect` dispatch on
the full modifier bitmask, so a multi-modifier event is never silently
absorbed by the first matching single-modifier rule. The TS helper only
reports the modifiers; the `Selection` resource decides what they mean.

## Out Of Scope

- **Game-specific HUD rendering** — the application still draws health bars,
  formation indicators, and custom selection treatments beyond the default
  rings.
- **Touch / gamepad input** — desktop mouse only.
- **Multi-rect / lasso selection** — single rectangle only.
- **Static-geometry BVH picking** — instanced render batches use
  `@three.ez/instanced-mesh` per-instance BVH today. Static terrain and other
  non-instanced meshes still use the raycaster path; `three-mesh-bvh` remains a
  future option for those meshes.

## Native Verification

`cargo run --example picking_demo -p galeon-engine` walks the data flow
against a 50-cube scene without a renderer attached: it spawns the cubes,
calls `Selection::apply_pick` / `apply_pick_rect` directly, and prints the
selection state after each step. This is the same code path the WASM bridge
drives in the browser.
