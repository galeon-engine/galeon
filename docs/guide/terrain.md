<!-- SPDX-License-Identifier: AGPL-3.0-only OR Commercial -->

# Heightmap Terrain

`galeon-engine-terrain` provides the Rust-owned terrain primitive for authored
heightmaps. The first surface is deliberately small: load or construct a dense
height grid, install it as a `Terrain` resource, and query height / normal data
from engine systems.

## Resource

```rust
use galeon_engine_terrain::{HeightmapPlugin, Terrain};
use galeon_engine::Engine;

let terrain = Terrain::new(
    [0.0, 0.0],  // origin in world X/Z
    [64.0, 64.0], // size in world X/Z
    65,
    65,
    vec![0.0; 65 * 65],
)?;

let mut engine = Engine::new();
engine.add_plugin(HeightmapPlugin::new(terrain));
```

`Terrain` height samples are row-major in local X/Z space. Sample columns move
in world +X, sample rows move in world +Z, and sample `(0, 0)` is located at
`origin`. The `size` argument spans from sample `(0, 0)` to
`(width - 1, height - 1)`, so adjacent sample spacing is
`size / (sample_count - 1)` per axis.

`height_at(x, z)` bilinearly samples the source grid and returns `None` outside
the terrain bounds. `normal_at(x, z)` estimates a normal from central
differences over the same source grid using the same world X/Z convention. This
query normal is for gameplay and sampling. The generated render mesh uses the
same first-pass finite-difference convention today, but the `TerrainMesh` buffer
is a render adapter rather than the authoritative data source.

Cloning a `Terrain` shares immutable height storage. `HeightmapPlugin` still
installs a normal `Terrain` resource, so systems can read `Res<Terrain>` without
an `Arc` wrapper, but plugin installation does not copy the full height buffer.

## PNG16 Loading

`Terrain::from_png16_reader` accepts 16-bit grayscale PNG data and maps each
sample from `height_min..height_max`. `vertical_exaggeration` scales the height
range while keeping `height_min` fixed.

DEM, GeoTIFF, tile streaming, and LOD are intentionally out of scope for this
engine primitive. Downstream applications can convert those sources into
authored PNG16 heightmaps before handing data to Galeon.

## Mesh Export

`TerrainMesh::from_terrain(&terrain)` generates a CPU-side triangle-list mesh
from the source samples:

- `positions()`: flat `[x, y, z]` triples, local to the terrain origin.
- `normals()`: flat `[x, y, z]` triples, pointing toward +Y for flat terrain.
- `indices()`: `u32` triangle indices with upward-facing winding.

The vertex grid preserves the same sample mapping: world +X maps to source
columns and world +Z maps to row-major source rows. A terrain with `width *
height` samples produces `width * height` vertices, or `(quad_count_x + 1) *
(quad_count_z + 1)`.

`HeightmapPlugin::with_render_mesh(mesh_handle, material_handle)` installs the
`TerrainMesh` resource and spawns one renderable terrain entity through the
existing `Transform` + `MeshHandle` + `MaterialHandle` extraction path. The
spawned entity is positioned at the terrain's X/Z origin. TypeScript consumers
still own converting `TerrainMesh` data into a Three.js `BufferGeometry` and
registering it against the chosen mesh handle; the `FramePacket` contract stays
unchanged.
