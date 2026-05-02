// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

use std::error::Error;
use std::io::Cursor;

use galeon_engine::{Engine, MaterialHandle, MeshHandle};
use galeon_engine_terrain::{HeightmapPlugin, Png16HeightmapOptions, Terrain, TerrainMesh};

fn main() -> Result<(), Box<dyn Error>> {
    let png = synthetic_ridge_png16(5, 5)?;
    let terrain = Terrain::from_png16_reader(
        Cursor::new(png),
        Png16HeightmapOptions {
            origin: [-2.0, -2.0],
            size: [4.0, 4.0],
            height_min: 0.0,
            height_max: 8.0,
            vertical_exaggeration: 1.0,
        },
    )?;

    let mut engine = Engine::new();
    engine.add_plugin(
        HeightmapPlugin::new(terrain)
            .with_render_mesh(MeshHandle { id: 100 }, MaterialHandle { id: 200 }),
    );

    let terrain = engine.world().resource::<Terrain>();
    let mesh = engine.world().resource::<TerrainMesh>();
    let packet = galeon_engine_three_sync::extract_frame(engine.world());

    assert_eq!(mesh.vertex_count(), 25);
    assert_eq!(packet.entity_count(), 1);
    assert_eq!(packet.mesh_handles[0], 100);
    assert_eq!(packet.material_handles[0], 200);

    let center_height = terrain.height_at(0.0, 0.0).unwrap();
    let center_normal = terrain.normal_at(0.0, 0.0).unwrap();
    assert!((center_height - 4.0).abs() < 0.01);
    assert!(center_normal[1] > 0.5);

    println!(
        "terrain example: vertices={}, entity={}, center_height={center_height:.2}, center_normal={center_normal:?}",
        mesh.vertex_count(),
        packet.entity_ids[0],
    );

    Ok(())
}

fn synthetic_ridge_png16(width: u32, height: u32) -> Result<Vec<u8>, png::EncodingError> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(png::ColorType::Grayscale);
        encoder.set_depth(png::BitDepth::Sixteen);
        let mut writer = encoder.write_header()?;
        let max = (width - 1 + height - 1) as f32;
        let mut data = Vec::with_capacity(width as usize * height as usize * 2);
        for z in 0..height {
            for x in 0..width {
                let raw = (((x + z) as f32 / max) * u16::MAX as f32).round() as u16;
                data.extend_from_slice(&raw.to_be_bytes());
            }
        }
        writer.write_image_data(&data)?;
    }
    Ok(bytes)
}
