# Effects — Particle / Billboard Primitives

Galeon ships a small generic effects toolkit:

| Layer | Crate / Package | What it provides |
|-------|-----------------|------------------|
| ECS | `galeon-engine::particle` | `Emitter`, `Particle`, `Billboard` components, distribution shapes, deterministic per-emitter RNG, spawn / expire system |
| Renderer | `@galeon/three` | Reference billboard `ShaderMaterial` (`createBillboardFbmMaterial`) — 2D-FBM density, lifetime fade, optional soft-edge depth fade |

Domain-specific shader content (smoke, fire, magic effects, water spray, etc.)
is **not** part of Galeon. The reference material is meant to be copied and
adapted by downstream applications.

## ECS Side: Emitters and Particles

```rust
use galeon_engine::{
    Engine, FloatDist, Vec3Dist, ColorDist,
    Emitter, Particle, Billboard,
    emitter_spawn_expire_system,
    Commands, FixedTimestep, QueryMut, Res,
};

let mut engine = Engine::new();
engine.set_tick_rate(30.0); // 30 Hz simulation

// Register the spawn / expire system on whichever stage you prefer.
engine.add_system::<(
    Res<'_, FixedTimestep>,
    QueryMut<'_, Emitter>,
    QueryMut<'_, Particle>,
    Commands<'_>,
)>("simulate", "particles", emitter_spawn_expire_system);

// Spawn an emitter entity.
engine.world_mut().spawn((
    Emitter::new(
        50.0,                                          // 50 particles/sec
        FloatDist::Uniform { min: 0.8, max: 1.4 },     // lifetime (s)
        2_000,                                         // max alive
    )
    .with_velocity(Vec3Dist::UniformBox {
        min: [-0.3, 0.5, -0.3],
        max: [ 0.3, 1.2,  0.3],
    })
    .with_size(FloatDist::Uniform { min: 0.4, max: 0.7 })
    .with_color(ColorDist::Constant([0.95, 0.92, 0.85]))
    .with_seed(0xC0FFEE),
));
```

### Determinism

Each `Emitter` carries its own `ParticleRng` (xorshift64 with SplitMix-style
seed mixing). Two emitters constructed with the same seed and ticked the
same way produce byte-for-byte identical particle streams. The engine has
no `rand` dependency.

### Cap behaviour

`Emitter::max` caps each emitter independently. When the cap is reached the
spawn debt (`spawn_accumulator`) is dropped to zero rather than carried
forward — this prevents a transient over-rate from producing a burst the
moment a slot frees up. Negative rates are clamped to zero, not allowed
to accumulate as negative debt.

## Renderer Side: Reference Billboard Material

`@galeon/three` exports `createBillboardFbmMaterial`. The factory builds a
`THREE.ShaderMaterial` configured for premultiplied-alpha transparent
billboards.

```ts
import * as THREE from "three";
import { createBillboardFbmMaterial } from "@galeon/three";

const material = createBillboardFbmMaterial({
  color: 0xf2ebd9,
  size: 1.5,
  noiseScale: 2.0,
  densityFalloff: 0.6,
});

const quad = new THREE.PlaneGeometry(1, 1);
const mesh = new THREE.Mesh(quad, material);
scene.add(mesh);

// Per-frame, drive the lifetime progress to fade the particle out.
function update(progress: number) {
  material.uniforms.uLifetimeProgress.value = progress; // 0 → 1
}
```

### Geometry assumption

The mesh wearing this material **must** use a unit `THREE.PlaneGeometry(1, 1)`
(positions in `[-0.5, 0.5]^2`). The vertex shader treats `position.xy` as the
quad-local offset and expects `uv` in `[0, 1]`. In the instanced path, the
same shader reads per-instance scale from `instanceMatrix` and keeps the quad
screen-aligned.

### Instanced billboard routing (T2)

The Rust extractor treats a renderable entity carrying `Billboard` as an
instanced billboard when it also has a `MeshHandle`. The mesh handle identifies
the shared unit-quad geometry. `RendererCache` then routes that row through
`InstancedMeshManager`. For materials created by `createBillboardFbmMaterial`,
batches are keyed by:

- `instance_groups[i]` (quad mesh group / `InstanceOf(MeshHandle)`)
- `material_handles[i]` (material/texture variant)

So 1000 billboards sharing one quad mesh handle and one material handle produce
one `THREE.InstancedMesh` batch; changing material handle splits batches without
thrashing a shared mesh material.

```rust
use galeon_engine::{Billboard, MaterialHandle, MeshHandle, Tint, Transform};

world.spawn((
    Transform::from_position(x, y, z),
    MeshHandle { id: 17 },                  // quad geometry
    MaterialHandle { id: 101 },             // billboard material/texture
    Billboard,                              // opt into billboard instancing
    Tint([1.0, 1.0, 1.0]),                  // optional per-instance tint
));
```

Use `InstanceOf(MeshHandle { id })` directly for non-billboard instancing. For
billboards, `Billboard` is the semantic marker; `MeshHandle` provides the batch
geometry key and `MaterialHandle` provides the material/texture split.

### Uniform reference

| Uniform | Type | Default | Purpose |
|---------|------|---------|---------|
| `uColor` | `vec3` | `(0.95, 0.92, 0.85)` | Linear RGB tint. |
| `uSize` | `float` | `1.0` | Quad size in world units. |
| `uLifetimeProgress` | `float` | `0.0` | `0` = newborn, `1` = expiring (alpha → 0). |
| `uNoiseScale` | `float` | `2.0` | UV multiplier into the FBM noise field. Higher = finer detail. |
| `uDensityFalloff` | `float` | `0.6` | `0` = uniform fill, `1` = density-dominated. |
| `uSoftEdgeDistance` | `float` | `0.0` | Soft-particle fade distance (world units) against scene depth. `0` disables. |
| `uHasDepthFade` | `float` | `0.0` | `1.0` when soft-edge fade is active. Set automatically by the factory. |
| `uDepthTexture` | `sampler2D` | `null` | Scene depth render target. Required when `uSoftEdgeDistance > 0`. |
| `uResolution` | `vec2` | `(1, 1)` | Render target resolution for screen-UV depth sampling. Update on resize. |
| `uCameraNear` / `uCameraFar` | `float` | `0.1 / 1000` | Camera clipping planes (used to linearize depth). |

The number of FBM octaves is compiled into the shader via a `defines` entry
(`OCTAVES = 4` by default, clamped to `[1, 8]`). Changing `noiseOctaves`
requires constructing a fresh material — Three.js does not recompile a
program when `defines` change.

### Soft-particle depth fade

To enable the soft-edge fade, pass a depth texture and a non-zero distance:

```ts
const depthRT = new THREE.WebGLRenderTarget(width, height, {
  depthTexture: new THREE.DepthTexture(width, height),
  depthBuffer: true,
});

const material = createBillboardFbmMaterial({
  softEdgeDistance: 0.5,
  depthTexture: depthRT.depthTexture,
  cameraNear: camera.near,
  cameraFar: camera.far,
  resolution: [width, height],
});

// Render scene to depthRT first (without billboards), then render the
// billboards with this material against the screen buffer.
```

Without the soft-edge fade, billboards intersecting world geometry produce a
hard "card cutting through wall" line. With it enabled, the alpha smoothly
goes to zero in the last `softEdgeDistance` units of approach.

### Premultiplied alpha

The fragment shader writes premultiplied alpha (`vec4(rgb * alpha, alpha)`).
The material configures `THREE.CustomBlending` with
`OneFactor` × `OneMinusSrcAlphaFactor` so it composites correctly regardless
of the renderer's `premultipliedAlpha` flag. **Do not** mix this material
with straight-alpha particle materials in the same emitter — the result is
the classic "blue-grey halo" bug.

## Adapting the Reference Shader

The reference fragment is intentionally compact. To adapt it for a specific
look, copy `packages/three/src/materials/billboard-fbm.ts` and edit:

- **`valueNoise` / `fbm`** — swap for simplex, gradient, or curl noise. The
  loop structure is unchanged.
- **`densityFalloff` math** — replace `radial * mix(1.0, density, ...)` with a
  domain-specific shaping function (e.g., a vertical gradient for rising
  smoke, a centred bright core for flame).
- **Lifetime fade curve** — the linear `1.0 - progress` is intentionally dull.
  Replace with `pow(1.0 - progress, 2.0)` or a piecewise curve for non-linear
  fade.
- **Color modulation by lifetime** — sample a gradient texture by
  `uLifetimeProgress` instead of using a flat `uColor`.

For higher-order effects (fire/sparks/magic), the discovery-thread references
(see issue [#217](https://github.com/galeon-engine/galeon/issues/217)) point
to Inigo Quilez's noise shadertoys and the
[three.quarks](https://github.com/Alchemist0823/three.quarks) BatchedRenderer
as starting points worth studying.

## Limitations and Out of Scope

- **Per-mesh frustum culling.** `THREE.InstancedMesh` evaluates frustum
  culling against a single bounding sphere — see the
  [#215](https://github.com/galeon-engine/galeon/issues/215) discussion. When
  rendering many billboards via `InstancedMesh`, set `frustumCulled = false`
  on the mesh.
- **Per-particle CPU sorting.** Above ~2000 transparent billboards, per-frame
  CPU sorting by depth is too expensive. Sort by emitter (one sort key per
  emitter group) instead.
- **GPU compute particle simulation.** The CPU spawn / expire system is
  WebGL2-friendly. A WebGPU compute path (transform feedback or storage
  buffers) is not in scope.
- **Mesh particles, collision-aware particles, ribbon trails.** Out of scope
  for the v1 primitive.

## See Also

- [`docs/guide/three-sync.md`](./three-sync.md) — render contract and the
  `FramePacket` / `RendererCache` pipeline.
- [`crates/engine/src/particle.rs`](../../crates/engine/src/particle.rs) —
  source of the ECS-side primitive.
- [`packages/three/src/materials/billboard-fbm.ts`](../../packages/three/src/materials/billboard-fbm.ts) —
  source of the reference material.
