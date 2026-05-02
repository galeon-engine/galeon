// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

use std::error::Error;
use std::fmt;
use std::io::{BufRead, Seek};
use std::sync::Arc;

use galeon_engine::{Engine, Plugin};

/// Heightfield sampled over the X/Z plane.
///
/// Heights are stored row-major from north-to-south in the heightmap's local
/// Z direction. `origin` is the world-space `[x, z]` coordinate of sample
/// `(0, 0)`, and `size` spans from sample `(0, 0)` to `(width - 1, height - 1)`.
#[derive(Debug, Clone, PartialEq)]
pub struct Terrain {
    origin: [f32; 2],
    size: [f32; 2],
    sample_count: [u32; 2],
    pixel_stride: [f32; 2],
    heights: Arc<[f32]>,
    min_height: f32,
    max_height: f32,
}

impl Terrain {
    /// Construct a terrain from row-major height samples.
    ///
    /// `width` and `height` are sample counts, not quad counts. Both must be at
    /// least `2`, and `heights.len()` must equal `width * height`.
    pub fn new(
        origin: [f32; 2],
        size: [f32; 2],
        width: u32,
        height: u32,
        heights: Vec<f32>,
    ) -> Result<Self, TerrainError> {
        if width < 2 || height < 2 {
            return Err(TerrainError::InvalidDimensions { width, height });
        }
        let expected = width as usize * height as usize;
        if heights.len() != expected {
            return Err(TerrainError::HeightCount {
                expected,
                actual: heights.len(),
            });
        }
        if !size[0].is_finite() || !size[1].is_finite() || size[0] <= 0.0 || size[1] <= 0.0 {
            return Err(TerrainError::InvalidSize { size });
        }
        if !origin[0].is_finite() || !origin[1].is_finite() {
            return Err(TerrainError::InvalidOrigin { origin });
        }
        if heights.iter().any(|h| !h.is_finite()) {
            return Err(TerrainError::NonFiniteHeight);
        }

        let (min_height, max_height) = min_max(&heights);
        Ok(Self {
            origin,
            size,
            sample_count: [width, height],
            pixel_stride: [size[0] / (width - 1) as f32, size[1] / (height - 1) as f32],
            heights: heights.into(),
            min_height,
            max_height,
        })
    }

    /// Load a 16-bit grayscale PNG heightmap.
    pub fn from_png16_reader<R: BufRead + Seek>(
        reader: R,
        options: Png16HeightmapOptions,
    ) -> Result<Self, TerrainError> {
        if !options.height_min.is_finite()
            || !options.height_max.is_finite()
            || !options.vertical_exaggeration.is_finite()
        {
            return Err(TerrainError::InvalidHeightScale);
        }
        if options.height_max < options.height_min {
            return Err(TerrainError::InvalidHeightRange {
                min: options.height_min,
                max: options.height_max,
            });
        }

        let decoder = png::Decoder::new(reader);
        let mut png_reader = decoder.read_info().map_err(TerrainError::PngDecode)?;
        let info = png_reader.info();
        if info.color_type != png::ColorType::Grayscale || info.bit_depth != png::BitDepth::Sixteen
        {
            return Err(TerrainError::UnsupportedPng {
                color_type: info.color_type,
                bit_depth: info.bit_depth,
            });
        }

        let buffer_size = png_reader
            .output_buffer_size()
            .ok_or(TerrainError::UnknownPngBufferSize)?;
        let mut bytes = vec![0; buffer_size];
        let frame = png_reader
            .next_frame(&mut bytes)
            .map_err(TerrainError::PngDecode)?;
        let data = &bytes[..frame.buffer_size()];
        if data.len() % 2 != 0 {
            return Err(TerrainError::MalformedPngData);
        }

        let height_range = options.height_max - options.height_min;
        let mut heights = Vec::with_capacity(data.len() / 2);
        for px in data.chunks_exact(2) {
            let raw = u16::from_be_bytes([px[0], px[1]]);
            let normalized = raw as f32 / u16::MAX as f32;
            heights.push(
                options.height_min + normalized * height_range * options.vertical_exaggeration,
            );
        }

        Self::new(
            options.origin,
            options.size,
            frame.width,
            frame.height,
            heights,
        )
    }

    /// World-space `[x, z]` origin of the first sample.
    pub fn origin(&self) -> [f32; 2] {
        self.origin
    }

    /// World-space `[x, z]` span covered by the terrain.
    pub fn size(&self) -> [f32; 2] {
        self.size
    }

    /// `[width, height]` sample counts.
    pub fn sample_count(&self) -> [u32; 2] {
        self.sample_count
    }

    /// Distance between adjacent samples in world units.
    pub fn pixel_stride(&self) -> [f32; 2] {
        self.pixel_stride
    }

    /// Raw row-major height samples.
    pub fn heights(&self) -> &[f32] {
        &self.heights
    }

    /// Minimum loaded height.
    pub fn min_height(&self) -> f32 {
        self.min_height
    }

    /// Maximum loaded height.
    pub fn max_height(&self) -> f32 {
        self.max_height
    }

    /// Axis-aligned bounds as `[min_x, min_y, min_z]` and `[max_x, max_y, max_z]`.
    pub fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        (
            [self.origin[0], self.min_height, self.origin[1]],
            [
                self.origin[0] + self.size[0],
                self.max_height,
                self.origin[1] + self.size[1],
            ],
        )
    }

    /// Bilinearly sample height at world-space `(x, z)`.
    ///
    /// Returns `None` outside the terrain bounds.
    pub fn height_at(&self, x: f32, z: f32) -> Option<f32> {
        let [u, v] = self.world_to_grid(x, z)?;
        Some(self.sample_bilinear(u, v))
    }

    /// Estimate the surface normal at world-space `(x, z)`.
    ///
    /// Uses central differences over the source heightfield and clamps neighbor
    /// taps at the terrain edge.
    pub fn normal_at(&self, x: f32, z: f32) -> Option<[f32; 3]> {
        let [u, v] = self.world_to_grid(x, z)?;
        let max_u = (self.sample_count[0] - 1) as f32;
        let max_v = (self.sample_count[1] - 1) as f32;
        let left_u = (u - 1.0).max(0.0);
        let right_u = (u + 1.0).min(max_u);
        let down_v = (v - 1.0).max(0.0);
        let up_v = (v + 1.0).min(max_v);

        let left = self.sample_bilinear(left_u, v);
        let right = self.sample_bilinear(right_u, v);
        let down = self.sample_bilinear(u, down_v);
        let up = self.sample_bilinear(u, up_v);

        let dx = (right_u - left_u) * self.pixel_stride[0];
        let dz = (up_v - down_v) * self.pixel_stride[1];
        let dhdx = (right - left) / dx;
        let dhdz = (up - down) / dz;
        normalize([-dhdx, 1.0, -dhdz])
    }

    fn world_to_grid(&self, x: f32, z: f32) -> Option<[f32; 2]> {
        if !x.is_finite() || !z.is_finite() {
            return None;
        }
        let max_x = self.origin[0] + self.size[0];
        let max_z = self.origin[1] + self.size[1];
        if x < self.origin[0] || x > max_x || z < self.origin[1] || z > max_z {
            return None;
        }
        Some([
            (x - self.origin[0]) / self.pixel_stride[0],
            (z - self.origin[1]) / self.pixel_stride[1],
        ])
    }

    fn sample_bilinear(&self, u: f32, v: f32) -> f32 {
        let max_x = (self.sample_count[0] - 1) as f32;
        let max_z = (self.sample_count[1] - 1) as f32;
        let u = u.clamp(0.0, max_x);
        let v = v.clamp(0.0, max_z);

        let x0 = u.floor() as u32;
        let z0 = v.floor() as u32;
        let x1 = (x0 + 1).min(self.sample_count[0] - 1);
        let z1 = (z0 + 1).min(self.sample_count[1] - 1);
        let tx = u - x0 as f32;
        let tz = v - z0 as f32;

        let h00 = self.sample_at(x0, z0);
        let h10 = self.sample_at(x1, z0);
        let h01 = self.sample_at(x0, z1);
        let h11 = self.sample_at(x1, z1);
        let a = h00 + (h10 - h00) * tx;
        let b = h01 + (h11 - h01) * tx;
        a + (b - a) * tz
    }

    fn sample_at(&self, x: u32, z: u32) -> f32 {
        self.heights[(z * self.sample_count[0] + x) as usize]
    }
}

/// PNG16 heightmap import options.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Png16HeightmapOptions {
    pub origin: [f32; 2],
    pub size: [f32; 2],
    pub height_min: f32,
    pub height_max: f32,
    pub vertical_exaggeration: f32,
}

impl Default for Png16HeightmapOptions {
    fn default() -> Self {
        Self {
            origin: [0.0, 0.0],
            size: [1.0, 1.0],
            height_min: 0.0,
            height_max: 1.0,
            vertical_exaggeration: 1.0,
        }
    }
}

/// Plugin that installs a loaded [`Terrain`] resource.
#[derive(Debug, Clone, PartialEq)]
pub struct HeightmapPlugin {
    terrain: Terrain,
}

impl HeightmapPlugin {
    pub fn new(terrain: Terrain) -> Self {
        Self { terrain }
    }
}

impl Plugin for HeightmapPlugin {
    fn build(&self, engine: &mut Engine) {
        engine.insert_resource(self.terrain.clone());
    }
}

#[derive(Debug)]
pub enum TerrainError {
    InvalidDimensions {
        width: u32,
        height: u32,
    },
    HeightCount {
        expected: usize,
        actual: usize,
    },
    InvalidOrigin {
        origin: [f32; 2],
    },
    InvalidSize {
        size: [f32; 2],
    },
    NonFiniteHeight,
    InvalidHeightScale,
    InvalidHeightRange {
        min: f32,
        max: f32,
    },
    UnsupportedPng {
        color_type: png::ColorType,
        bit_depth: png::BitDepth,
    },
    UnknownPngBufferSize,
    MalformedPngData,
    PngDecode(png::DecodingError),
}

impl fmt::Display for TerrainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDimensions { width, height } => {
                write!(
                    f,
                    "terrain dimensions must be at least 2x2, got {width}x{height}"
                )
            }
            Self::HeightCount { expected, actual } => {
                write!(
                    f,
                    "terrain expected {expected} height samples, got {actual}"
                )
            }
            Self::InvalidOrigin { origin } => {
                write!(f, "terrain origin must be finite, got {origin:?}")
            }
            Self::InvalidSize { size } => {
                write!(f, "terrain size must be finite and positive, got {size:?}")
            }
            Self::NonFiniteHeight => write!(f, "terrain heights must be finite"),
            Self::InvalidHeightScale => write!(f, "PNG height scale values must be finite"),
            Self::InvalidHeightRange { min, max } => {
                write!(f, "PNG height_max must be >= height_min, got {max} < {min}")
            }
            Self::UnsupportedPng {
                color_type,
                bit_depth,
            } => write!(
                f,
                "heightmap PNG must be 16-bit grayscale, got {color_type:?} {bit_depth:?}",
            ),
            Self::UnknownPngBufferSize => {
                write!(f, "PNG decoder did not report output buffer size")
            }
            Self::MalformedPngData => write!(f, "PNG frame data had an odd byte count"),
            Self::PngDecode(err) => write!(f, "PNG decode failed: {err}"),
        }
    }
}

impl Error for TerrainError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::PngDecode(err) => Some(err),
            _ => None,
        }
    }
}

fn min_max(values: &[f32]) -> (f32, f32) {
    let mut min = f32::INFINITY;
    let mut max = f32::NEG_INFINITY;
    for value in values {
        min = min.min(*value);
        max = max.max(*value);
    }
    (min, max)
}

fn normalize(v: [f32; 3]) -> Option<[f32; 3]> {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len <= f32::EPSILON {
        return None;
    }
    Some([v[0] / len, v[1] / len, v[2] / len])
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    fn synthetic_4x4() -> Terrain {
        Terrain::new(
            [10.0, 20.0],
            [3.0, 3.0],
            4,
            4,
            vec![
                0.0, 1.0, 2.0, 3.0, //
                1.0, 2.0, 3.0, 4.0, //
                2.0, 3.0, 4.0, 5.0, //
                3.0, 4.0, 5.0, 6.0,
            ],
        )
        .unwrap()
    }

    #[test]
    fn height_at_bilinear_samples_synthetic_grid() {
        let terrain = synthetic_4x4();

        assert_eq!(terrain.height_at(10.0, 20.0), Some(0.0));
        assert_eq!(terrain.height_at(13.0, 23.0), Some(6.0));
        assert_eq!(terrain.height_at(11.5, 21.5), Some(3.0));
        assert_eq!(terrain.height_at(9.9, 20.0), None);
    }

    #[test]
    fn normal_at_uses_source_height_gradient() {
        let terrain = synthetic_4x4();
        let normal = terrain.normal_at(11.0, 21.0).unwrap();
        let expected = [
            -1.0 / 3.0_f32.sqrt(),
            1.0 / 3.0_f32.sqrt(),
            -1.0 / 3.0_f32.sqrt(),
        ];

        assert!((normal[0] - expected[0]).abs() < 1e-6);
        assert!((normal[1] - expected[1]).abs() < 1e-6);
        assert!((normal[2] - expected[2]).abs() < 1e-6);
    }

    #[test]
    fn normal_at_preserves_planar_gradient_near_edges() {
        let terrain = synthetic_4x4();
        let interior = terrain.normal_at(11.0, 21.0).unwrap();
        let near_max_edge = terrain.normal_at(12.9, 22.9).unwrap();

        for i in 0..3 {
            assert!(
                (near_max_edge[i] - interior[i]).abs() < 1e-6,
                "component {i}: expected {interior:?}, got {near_max_edge:?}",
            );
        }
    }

    #[test]
    fn bounds_and_stride_reflect_loaded_grid() {
        let terrain = synthetic_4x4();

        assert_eq!(terrain.sample_count(), [4, 4]);
        assert_eq!(terrain.pixel_stride(), [1.0, 1.0]);
        assert_eq!(terrain.bounds(), ([10.0, 0.0, 20.0], [13.0, 6.0, 23.0]));
    }

    #[test]
    fn heightmap_plugin_inserts_terrain_resource() {
        let terrain = synthetic_4x4();
        let height_storage = terrain.heights().as_ptr();
        let mut engine = Engine::new();

        engine.add_plugin(HeightmapPlugin::new(terrain.clone()));

        assert_eq!(engine.world().resource::<Terrain>(), &terrain);
        assert_eq!(
            engine.world().resource::<Terrain>().heights().as_ptr(),
            height_storage
        );
    }

    #[test]
    fn terrain_clone_shares_immutable_height_storage() {
        let terrain = synthetic_4x4();
        let clone = terrain.clone();

        assert_eq!(clone, terrain);
        assert_eq!(clone.heights().as_ptr(), terrain.heights().as_ptr());
    }

    #[test]
    fn png16_loader_decodes_fixture_corner_values() {
        let bytes = include_bytes!("../tests/fixtures/heightmap-16x16-gray16.png");
        let terrain = Terrain::from_png16_reader(
            Cursor::new(bytes.as_slice()),
            Png16HeightmapOptions {
                origin: [-8.0, -8.0],
                size: [15.0, 15.0],
                height_min: -10.0,
                height_max: 10.0,
                vertical_exaggeration: 1.5,
            },
        )
        .unwrap();

        assert_eq!(terrain.sample_count(), [16, 16]);
        assert_eq!(terrain.pixel_stride(), [1.0, 1.0]);
        assert!((terrain.height_at(-8.0, -8.0).unwrap() - -10.0).abs() < 1e-6);
        assert!((terrain.height_at(7.0, 7.0).unwrap() - 20.0).abs() < 1e-6);
        assert_eq!(terrain.min_height(), -10.0);
        assert_eq!(terrain.max_height(), 20.0);
    }

    #[test]
    fn png16_loader_rejects_non_gray16_png() {
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, 1, 1);
            encoder.set_color(png::ColorType::Grayscale);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&[0]).unwrap();
        }

        let err = Terrain::from_png16_reader(Cursor::new(bytes), Png16HeightmapOptions::default())
            .unwrap_err();
        assert!(matches!(err, TerrainError::UnsupportedPng { .. }));
    }

    #[test]
    fn sample_bilinear_clamps_coordinates_above_max_index() {
        let terrain = synthetic_4x4();

        let height = terrain.sample_bilinear(4.0, 4.0);

        assert_eq!(height, 6.0);
    }
}
