# Instanced Cubes Benchmark

5000 cubes drifting in a sine field — exercises the GPU-instanced render
path added in issue #215 and provides a side-by-side comparison against the
standalone-`Object3D` path.

## Run

```bash
bun install
bun --cwd examples/instanced-cubes run build
# Serve the directory with any static file server, e.g.
bunx serve examples/instanced-cubes
# Then open http://localhost:3000/index.html in a browser.
```

The page boots with `?mode=instanced` by default. Append `?mode=standalone`
to switch the same 5000 entities into the per-entity `THREE.Mesh` render
path that existed before issue #215.

## What it measures

The simulation cost is intentionally tiny — a sine-field position update
that runs in microseconds for 5000 entities. The renderer cost dominates,
so the FPS readout reflects the cost of:

- 5000 `setMatrixAt` writes on a single `THREE.InstancedMesh` (instanced)
  vs. 5000 individual `Object3D.position`/`quaternion`/`scale` updates
  followed by 5000 draw calls (standalone).
- One draw call vs. 5000 draw calls per frame.

For a fair comparison the example marks every entity as
`CHANGED_TRANSFORM` every frame in both modes, so the renderer never
short-circuits on unchanged data.

## Recording your numbers

Document your hardware and your readings in
[`docs/guide/instancing.md`](../../docs/guide/instancing.md). Numbers vary
significantly by GPU, browser version, and pixel ratio — please do not
quote anyone else's readings as your own.
