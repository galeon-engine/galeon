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

`height_at(x, z)` bilinearly samples the source grid and returns `None` outside
the terrain bounds. `normal_at(x, z)` estimates a normal from central
differences over the same source grid.

## PNG16 Loading

`Terrain::from_png16_reader` accepts 16-bit grayscale PNG data and maps each
sample from `height_min..height_max`. `vertical_exaggeration` scales the height
range while keeping `height_min` fixed.

DEM, GeoTIFF, tile streaming, and LOD are intentionally out of scope for this
engine primitive. Downstream applications can convert those sources into
authored PNG16 heightmaps before handing data to Galeon.
