// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

use galeon_engine::render::{MaterialHandle, MeshHandle, Transform, Visibility};
use galeon_engine::{Entity, RenderChannelRegistry, World};

use crate::frame_packet::{ChannelData, FramePacket};

/// Extract render-facing data from the ECS world into a packed frame packet.
///
/// Iterates all entities with a `Transform` component (the implicit "renderable"
/// marker) and packs their transform, visibility, mesh, and material data into
/// flat arrays suitable for WASM transport.
///
/// If a [`RenderChannelRegistry`] resource is present, also extracts all
/// registered custom channels into `FramePacket::custom_channels`.
///
/// Missing optional components use sensible defaults:
/// - `Visibility`: defaults to visible (`true`)
/// - `MeshHandle`: defaults to `0` (no mesh)
/// - `MaterialHandle`: defaults to `0` (no material)
/// - Custom channels: defaults to `0.0` for all floats
type Renderable = (Entity, [f32; 3], [f32; 4], [f32; 3]);

pub fn extract_frame(world: &World) -> FramePacket {
    // First pass: collect entity IDs and transform data into owned values.
    // The `QueryIter` borrows `world.archetypes` for its lifetime; `.collect()`
    // consumes the iterator and releases that borrow, so we can call
    // `world.get()` per entity in the second pass.
    let renderables: Vec<Renderable> = world
        .query::<&Transform>()
        .map(|(e, t)| (e, t.position, t.rotation, t.scale))
        .collect();

    let mut packet = FramePacket::with_capacity(renderables.len());
    let mut entities = Vec::with_capacity(renderables.len());

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

        entities.push(*entity);
    }

    // Extract registered custom channels.
    if let Some(registry) = world.try_resource::<RenderChannelRegistry>() {
        for channel in &registry.channels {
            let mut data = vec![0.0f32; entities.len() * channel.stride];
            for (i, entity) in entities.iter().enumerate() {
                let offset = i * channel.stride;
                (channel.extract_fn)(world, *entity, &mut data[offset..offset + channel.stride]);
            }
            packet.custom_channels.insert(
                channel.name.clone(),
                ChannelData {
                    stride: channel.stride,
                    data,
                },
            );
        }
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

    // -------------------------------------------------------------------------
    // Custom channel extraction
    // -------------------------------------------------------------------------

    #[derive(Debug)]
    struct WearState {
        wear: f32,
        heat: f32,
    }

    impl galeon_engine::Component for WearState {}

    impl galeon_engine::ExtractToFloats for WearState {
        const STRIDE: usize = 2;
        fn extract(&self, buf: &mut [f32]) {
            buf[0] = self.wear;
            buf[1] = self.heat;
        }
    }

    #[test]
    fn extract_no_channels_when_registry_absent() {
        let mut world = World::new();
        world.spawn((Transform::identity(),));
        let packet = extract_frame(&world);
        assert_eq!(packet.channel_count(), 0);
    }

    #[test]
    fn extract_custom_channel_for_all_entities() {
        let mut world = World::new();
        let mut reg = galeon_engine::RenderChannelRegistry::new();
        reg.register::<WearState>("wear");
        world.insert_resource(reg);

        world.spawn((
            Transform::identity(),
            WearState {
                wear: 0.5,
                heat: 0.3,
            },
        ));
        world.spawn((
            Transform::from_position(1.0, 0.0, 0.0),
            WearState {
                wear: 0.8,
                heat: 0.1,
            },
        ));

        let packet = extract_frame(&world);
        assert_eq!(packet.entity_count(), 2);
        assert_eq!(packet.channel_count(), 1);

        let ch = packet.channel("wear").unwrap();
        assert_eq!(ch.stride, 2);
        assert_eq!(ch.data.len(), 4);
        assert!((ch.data[0] - 0.5).abs() < f32::EPSILON);
        assert!((ch.data[1] - 0.3).abs() < f32::EPSILON);
        assert!((ch.data[2] - 0.8).abs() < f32::EPSILON);
        assert!((ch.data[3] - 0.1).abs() < f32::EPSILON);
    }

    #[test]
    fn extract_channel_zeroes_when_component_absent() {
        let mut world = World::new();
        let mut reg = galeon_engine::RenderChannelRegistry::new();
        reg.register::<WearState>("wear");
        world.insert_resource(reg);

        world.spawn((
            Transform::identity(),
            WearState {
                wear: 0.5,
                heat: 0.3,
            },
        ));
        world.spawn((Transform::from_position(1.0, 0.0, 0.0),));

        let packet = extract_frame(&world);
        assert_eq!(packet.entity_count(), 2);

        let ch = packet.channel("wear").unwrap();
        assert!((ch.data[0] - 0.5).abs() < f32::EPSILON);
        assert!((ch.data[1] - 0.3).abs() < f32::EPSILON);
        assert!((ch.data[2]).abs() < f32::EPSILON);
        assert!((ch.data[3]).abs() < f32::EPSILON);
    }

    #[test]
    fn extract_empty_channel_when_no_entities() {
        let mut world = World::new();
        let mut reg = galeon_engine::RenderChannelRegistry::new();
        reg.register::<WearState>("wear");
        world.insert_resource(reg);

        let packet = extract_frame(&world);
        assert_eq!(packet.entity_count(), 0);
        let ch = packet.channel("wear").unwrap();
        assert!(ch.data.is_empty());
    }
}
