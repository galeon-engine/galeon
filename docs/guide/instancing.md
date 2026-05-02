# GPU-Instanced Rendering

Galeon's renderer can route any entity through a shared
`THREE.InstancedMesh` instead of allocating its own `Object3D`. This is the
right path for crowd-scale scenes — 1000+ entities sharing a single
geometry, where per-entity draw calls become the bottleneck.

The instanced path is opt-in per entity. The standalone-`Object3D` path
remains the default for scene-graph hierarchies, parented children,
mixed-type composites, and anything that is not a "many of the same shape"
case.

## When to use it

Use `InstanceOf` when **all** of these are true:

- You have hundreds-to-thousands of entities sharing **the same mesh**.
- The instances do not need scene-graph parenting (instanced entities live
  on the scene root in v1).
- Per-instance variation is limited to transform and (optionally) tint.

Skip it when:

- Each entity has its own geometry — instancing buys nothing if you would
  end up with one batch per entity.
- You need transform inheritance from a parent entity.
- You need per-instance materials. (V1 supports per-instance color via
  `Tint`. Per-instance shader uniforms are out of scope.)

## ECS surface

```rust
use galeon_engine::{InstanceOf, MeshHandle, Tint, Transform};

world.spawn((
    Transform::from_position(x, y, z),
    MeshHandle { id: 7 },                  // shared geometry
    InstanceOf(MeshHandle { id: 7 }),      // join the batch keyed by 7
    Tint([0.1, 0.6, 0.9]),                 // optional per-instance colour
));
```

The wrapped `MeshHandle` is the **batch key**. Two entities with the same
`InstanceOf(handle)` share one `THREE.InstancedMesh`. The renderer creates
the batch lazily on first sighting, grows its capacity by 2× when full,
and reuses freed slots via swap-with-last on remove.

## Render-snapshot surface

| Channel | Type | Meaning |
|---------|------|---------|
| `instance_groups` | `Vec<u32>` (Rust) / `Uint32Array` (TS) | Per entity. `INSTANCE_GROUP_NONE` (`0xFFFFFFFF`) → standalone `Object3D`. Any other value → that batch's `InstancedMesh`. |
| `tints` | `Vec<f32>` / `Float32Array`, stride 3 | Per entity, linear sRGB `[r, g, b]`. White (`[1, 1, 1]`) is the no-op identity. |
| `CHANGED_INSTANCE_GROUP` | flag bit `1 << 6` | Set on incremental packets when an entity migrates between batches or in/out of the instanced path. |
| `CHANGED_TINT` | flag bit `1 << 7` | Set on incremental packets when `Tint` is added, removed, or mutated. |

The TS-side `RendererCache` consumes these directly; consumer code does not
need to do anything beyond registering geometries / materials. Standalone
entities and instanced entities can coexist freely in the same world and
the same packet — `RendererCache` routes them into the right path each
frame and migrates them on the change-flag bits.

## Performance characteristics

Instancing collapses N transform updates into a single
`InstancedMesh.instanceMatrix` upload and one draw call. Compared to N
individual `Object3D` updates plus N draw calls, savings scale roughly
linearly with N once draw-call overhead dominates frame time (typically
above ~500 entities on modern hardware).

Caveats:

- **Frustum culling is disabled per `InstancedMesh`** because three.js
  treats the whole batch as visible-or-not (issue mrdoob/three.js#27170).
  A future per-instance backend (BVH or `BatchedMesh`) is out of scope
  for this iteration.
- **`instanceColor` is allocated synchronously at batch creation**, even
  when no entity is tinted. This avoids the shader-recompile cliff from
  three.js issue #21786 (first non-null write triggers a recompile). The
  cost is `capacity * 12` bytes per batch.
- **Hidden instances render at zero scale**. The slot stays put in the
  active range — cheap re-show, no shuffling. If you toggle visibility
  more often than batch composition changes, this is the right trade.

## The benchmark example

[`examples/instanced-cubes`](../../examples/instanced-cubes) drives 5000
cubes through a sine field. The same simulation runs in two modes via the
URL query parameter:

```
http://localhost:3000/?mode=instanced     # default
http://localhost:3000/?mode=standalone
```

In both modes the example marks every entity as `CHANGED_TRANSFORM` every
frame so the renderer never short-circuits on unchanged data — the
comparison reflects the worst-case (full-update) cost.

### Methodology

1. Load the page, wait ~2 s for the FPS readout to stabilise.
2. Note the rolling-average FPS over the last 60 frames as displayed in
   the HUD.
3. Repeat with the other mode.
4. Record both numbers along with your hardware in the table below.

Numbers vary significantly by GPU, browser, pixel ratio, and window size.
Do not quote anyone else's numbers as your own.

### Recording your numbers

Append a row to the table when you run the benchmark on your machine.

| Date | Hardware | Browser | Resolution | Instanced FPS | Standalone FPS | Speed-up |
|------|----------|---------|-----------:|--------------:|---------------:|---------:|
| _yyyy-mm-dd_ | _e.g. M2 Pro / RTX 4070_ | _e.g. Chrome 138_ | _e.g. 2560×1440_ | _filled in_ | _filled in_ | _filled in_ |

> **Why blank?** Issue #215 deliberately leaves the numbers cell blank.
> The verification ("Manual perf comparison") is meant to be done on the
> machine that consumes the engine. Pre-filling the table with one
> machine's readings would give a false sense of portability.
