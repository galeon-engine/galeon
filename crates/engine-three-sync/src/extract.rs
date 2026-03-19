// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

use galeon_engine::World;
use galeon_engine::render::{MaterialHandle, MeshHandle, Transform, Visibility};

use crate::frame_packet::FramePacket;

/// Extract render-facing data from the ECS world into a packed frame packet.
///
/// Iterates all entities with a `Transform` component (the implicit "renderable"
/// marker) and packs their transform, visibility, mesh, and material data into
/// flat arrays suitable for WASM transport.
///
/// Missing optional components use sensible defaults:
/// - `Visibility`: defaults to visible (`true`)
/// - `MeshHandle`: defaults to `0` (no mesh)
/// - `MaterialHandle`: defaults to `0` (no material)
type Renderable = (galeon_engine::Entity, [f32; 3], [f32; 4], [f32; 3]);

pub fn extract_frame(world: &World) -> FramePacket {
    // First pass: collect entity IDs and transform data into owned values.
    // This releases the borrow on `world` so we can call `get()` per entity.
    let renderables: Vec<Renderable> = world
        .query::<Transform>()
        .iter()
        .map(|(e, t)| (*e, t.position, t.rotation, t.scale))
        .collect();

    let mut packet = FramePacket::with_capacity(renderables.len());

    for (entity, position, rotation, scale) in &renderables {
        let visible = world
            .get::<Visibility>(*entity)
            .map(|v| v.visible)
            .unwrap_or(true);

        let mesh_id = world.get::<MeshHandle>(*entity).map(|m| m.id).unwrap_or(0);

        let material_id = world
            .get::<MaterialHandle>(*entity)
            .map(|m| m.id)
            .unwrap_or(0);

        packet.push(
            entity.index(),
            entity.generation(),
            position,
            rotation,
            scale,
            visible,
            mesh_id,
            material_id,
        );
    }

    packet
}

#[cfg(test)]
mod tests {
    use super::*;
    use galeon_engine::render::{MaterialHandle, MeshHandle, Transform, Visibility};

    #[test]
    fn extract_empty_world() {
        let world = World::new();
        let packet = extract_frame(&world);
        assert_eq!(packet.entity_count(), 0);
    }

    #[test]
    fn extract_entity_with_all_components() {
        let mut world = World::new();
        world.spawn((
            Transform::from_position(1.0, 2.0, 3.0),
            Visibility { visible: true },
            MeshHandle { id: 10 },
            MaterialHandle { id: 20 },
        ));

        let packet = extract_frame(&world);
        assert_eq!(packet.entity_count(), 1);
        assert_eq!(packet.transforms[0], 1.0); // pos.x
        assert_eq!(packet.transforms[1], 2.0); // pos.y
        assert_eq!(packet.transforms[2], 3.0); // pos.z
        assert_eq!(packet.visibility[0], 1);
        assert_eq!(packet.mesh_handles[0], 10);
        assert_eq!(packet.material_handles[0], 20);
    }

    #[test]
    fn extract_defaults_for_missing_components() {
        let mut world = World::new();
        // Only Transform — no Visibility, MeshHandle, MaterialHandle
        world.spawn((Transform::identity(),));

        let packet = extract_frame(&world);
        assert_eq!(packet.entity_count(), 1);
        assert_eq!(packet.visibility[0], 1); // default: visible
        assert_eq!(packet.mesh_handles[0], 0); // default: 0
        assert_eq!(packet.material_handles[0], 0); // default: 0
    }

    #[test]
    fn extract_skips_entities_without_transform() {
        let mut world = World::new();
        // Entity with transform
        world.spawn((Transform::identity(), MeshHandle { id: 1 }));
        // Entity without transform — should NOT appear in packet
        world.spawn((Visibility { visible: false },));

        let packet = extract_frame(&world);
        assert_eq!(packet.entity_count(), 1);
        assert_eq!(packet.mesh_handles[0], 1);
    }

    #[test]
    fn extract_invisible_entity() {
        let mut world = World::new();
        world.spawn((Transform::identity(), Visibility { visible: false }));

        let packet = extract_frame(&world);
        assert_eq!(packet.entity_count(), 1);
        assert_eq!(packet.visibility[0], 0); // false
    }

    #[test]
    fn extract_includes_entity_generation() {
        let mut world = World::new();
        let e = world.spawn((Transform::identity(),));
        // First entity gets generation 0.
        let packet = extract_frame(&world);
        assert_eq!(packet.entity_generations[0], 0);

        // Despawn and spawn again — slot reused with bumped generation.
        world.despawn(e);
        world.spawn((Transform::identity(),));
        let packet = extract_frame(&world);
        assert_eq!(packet.entity_ids[0], 0); // same slot index
        assert_eq!(packet.entity_generations[0], 1); // bumped generation
    }

    #[test]
    fn extract_multiple_entities() {
        let mut world = World::new();
        world.spawn((
            Transform::from_position(1.0, 0.0, 0.0),
            MeshHandle { id: 1 },
        ));
        world.spawn((
            Transform::from_position(2.0, 0.0, 0.0),
            MeshHandle { id: 2 },
        ));
        world.spawn((
            Transform::from_position(3.0, 0.0, 0.0),
            MeshHandle { id: 3 },
        ));

        let packet = extract_frame(&world);
        assert_eq!(packet.entity_count(), 3);
    }
}
