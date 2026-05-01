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

These follow the StarCraft / OpenRA consensus catalogued in the discovery
notes for issue #214. The TS helper only reports the modifiers; the
`Selection` resource decides what they mean.

## Out Of Scope

- **Selection HUD rendering** — the application draws highlight rings,
  health bars, formation indicators.
- **Touch / gamepad input** — desktop mouse only.
- **Multi-rect / lasso selection** — single rectangle only.
- **GPU-accelerated picking** — when scenes scale past what raycasting can
  handle, look at `@three.ez/instanced-mesh` (per-instance BVH) or
  `three-mesh-bvh` (static geometry). The discovery notes have details.

## Native Verification

`cargo run --example picking_demo -p galeon-engine` walks the data flow
against a 50-cube scene without a renderer attached: it spawns the cubes,
calls `Selection::apply_pick` / `apply_pick_rect` directly, and prints the
selection state after each step. This is the same code path the WASM bridge
drives in the browser.
