# ADR 0003: Mouse Picking And Drag-Rectangle Selection Helper

## Status

Accepted

## Context

Every interactive Galeon project (RTS-style scenes, sandboxes, editor tooling)
reimplements the same two pieces of mouse plumbing against Three.js: mouse-to-ray
picking against the scene, and drag-rectangle (marquee) selection. Both reach
back into the ECS to identify hit entities. Without a shared helper the math
gets re-derived every time and tends to drift in subtle ways — NDC pixel math
that breaks for non-fullscreen canvases, AABB-vs-rect tests that produce false
positives, modifier-key conventions that disagree between sibling projects.

The render contract from ADR 0002 already exposes a stable per-entity stamp on
managed Three.js objects (`userData[GALEON_ENTITY_KEY] = { entityId, generation }`),
so a picking helper has a clear seam to resolve hits back to ECS entities. What
was missing was the helper itself and a uniform place on the Rust side to hold
the resulting selection state so game systems can react to it.

## Decision

Picking is a thin TypeScript helper plus a thin Rust resource, with the WASM
bridge ferrying typed events between them.

### TypeScript: `@galeon/picking`

A new framework-neutral package owns the input + raycasting logic.
`attachPicking(canvas, scene, camera, opts)` wires `mousedown` / `mousemove` /
`mouseup` / `mouseleave` listeners and emits two event kinds:

- `{ kind: "pick", entity, point, modifiers }` for clicks below the drag
  threshold. The first ray intersection whose ancestor chain carries a
  `GALEON_ENTITY_KEY` stamp wins; `entity` is `null` if no managed object is hit.
- `{ kind: "pick-rect", entities, modifiers }` for drags past the threshold.
  All managed entities whose world-space AABB intersects a six-plane sub-frustum
  derived from the rect are reported.

Both paths force `scene.updateMatrixWorld(true)` and
`camera.updateMatrixWorld()` first so input handled between a camera move and
the next render still sees current world transforms (`scene.updateMatrixWorld`
does not touch a camera that lives outside the scene graph), and both skip
hidden entities — the click path filters intersections via the visible
ancestor chain, and the marquee path walks the scene tree manually so an
invisible parent prunes its entire subtree. A stamped `THREE.Group`'s AABB is
computed by unioning the world-space boxes of its visible descendant geometry
through a manual recursive walker (not `Object3D.traverse`, which would still
descend into hidden subtrees and let visible grandchildren under a hidden
ancestor enlarge the AABB beyond what the renderer would draw). Grouped
entities with offset child meshes therefore marquee-select by the bounds the
viewer can actually see, rather than by the group origin or by hidden
geometry.

The marquee frustum is the canonical `SelectionBox.js` algorithm from three.js
examples (unproject the four NDC corners through the camera at near/far,
build planes via `Plane.setFromCoplanarPoints`), with one safety addition: each
plane normal is flipped if it does not point toward the centroid of the eight
corners. That sidesteps the camera-handedness traps the textbook orderings hit
when used outside their tested perspective-looking-down-`-Z` setup.

NDC math uses `getBoundingClientRect()` (not `innerWidth` / `innerHeight`) so
non-fullscreen canvases work, and DPR is intentionally not applied — NDC is
unitless. These pitfalls are catalogued in the issue #214 discovery notes.

### Rust: `Selection` resource in `galeon-engine`

A new world-global resource carries the current selection plus the last hit
point. Two methods apply input events:

- `Selection::apply_pick(entity, point, modifiers)` — single click.
  - No modifier replaces (or clears on a miss).
  - Shift only: toggles. Misses are no-ops to avoid accidental clears mid-shift.
  - Ctrl only: subtracts. Misses are no-ops.
  - Any other combination (Shift+Ctrl, Alt, Meta, …): same as no-modifier on a
    hit (replace), no-op on a miss. Dispatch is on the full modifier bitmask
    so a multi-modifier click does not get absorbed by the first matching
    single-modifier rule.
- `Selection::apply_pick_rect(entities, modifiers)` — marquee.
  - No modifier replaces. `shift` adds. `ctrl` subtracts. `alt` intersects.

Modifier semantics follow the StarCraft / OpenRA consensus catalogued in the
discovery notes; the helper only reports modifiers, the resource decides what
they mean. The `Selection` resource does not dispatch any commands itself —
game systems read it through `Res<Selection>` and react in their own systems.

### WASM bridge

`engine-three-sync` exposes three new methods on `WasmEngine`: `applyPick`,
`applyPickRect`, and `selectionEntities`. The first two lazy-install the
`Selection` resource on first use so consumers do not have to register it
explicitly. Modifier flags are passed as a single `u32` bitmask (shift = 1,
ctrl = 2, alt = 4, meta = 8) to avoid serialising a struct across the JS
boundary.

## Consequences

Interactive Galeon projects no longer need to write picking math. They can
mount `attachPicking` against their canvas, hand the events to the WASM bridge,
and read selection state from the ECS. The split keeps Three.js-specific
ray/frustum work in TypeScript (matching ADR 0002) while keeping the
authoritative selection state in Rust where game systems already live.

Out of scope for this iteration: selection HUD rendering (the application's
job), touch and gamepad input, and multi-frustum selection (only single-rect).
GPU-accelerated picking for large instanced scenes is a known follow-up — the
discovery notes flag `@three.ez/instanced-mesh` and `three-mesh-bvh` as the
references to study when that becomes a bottleneck.
