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

Instanced render batches also resolve to Galeon entity handles for click
picking. `@galeon/three` stamps each `THREE.InstancedMesh` with an
`instanceId -> { entityId, generation }` resolver, and `@galeon/picking` uses
the `THREE.Raycaster` intersection's `instanceId` before falling back to the
normal ancestor-stamp path. Marquee selection still uses object AABBs; large
per-instance marquee acceleration remains a follow-up under #224.

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
`getObject(entityId, generation)` target. It intentionally uses per-entity
wire rings instead of a post-processing `OutlinePass`, so consumers do not need
to adopt `EffectComposer` just to show selection. Call `update(selection)` after
selection changes or once per render frame if selected objects keep moving.

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
- **GPU-accelerated picking** — when scenes scale past what raycasting can
  handle, look at `@three.ez/instanced-mesh` (per-instance BVH) or
  `three-mesh-bvh` (static geometry). The default click path already preserves
  instanced entity identity through `Intersection.instanceId`; faster backends
  must preserve the same `{ entityId, generation }` result shape.

## Native Verification

`cargo run --example picking_demo -p galeon-engine` walks the data flow
against a 50-cube scene without a renderer attached: it spawns the cubes,
calls `Selection::apply_pick` / `apply_pick_rect` directly, and prints the
selection state after each step. This is the same code path the WASM bridge
drives in the browser.
