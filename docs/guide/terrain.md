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
query normal is for gameplay and sampling; render mesh normals belong to the
future mesh builder.

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

## Mesh Export Convention

When terrain mesh export is added, it must preserve the same sample mapping:
world +X maps to source columns and world +Z maps to row-major source rows. The
mesh contract must document vertex order, winding, and normal direction, and
must keep generated render normals separate from `normal_at` source-query
normals.
