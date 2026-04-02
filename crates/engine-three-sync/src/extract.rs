// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

use galeon_engine::render::{MaterialHandle, MeshHandle, Transform, Visibility};
use galeon_engine::{Entity, RenderChannelRegistry, World};

use crate::frame_packet::{
    CHANGED_MATERIAL, CHANGED_MESH, CHANGED_TRANSFORM, CHANGED_VISIBILITY, ChannelData,
    FramePacket,
};

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

/// Incremental extraction: only entities whose renderable components changed
/// since `since_tick`. Each entity gets a `change_flags` bitmask indicating
/// which fields changed.
///
/// Entities whose Transform was added or changed are always included.
/// The change flags further indicate if Visibility, MeshHandle, or
/// MaterialHandle changed on those entities.
pub fn extract_frame_incremental(world: &World, since_tick: u64) -> FramePacket {
    // Collect changed Transform entities first (releases archetype borrow).
    let renderables: Vec<Renderable> = world
        .query_changed::<Transform>(since_tick)
        .map(|(e, t)| (e, t.position, t.rotation, t.scale))
        .collect();

    let mut packet = FramePacket::with_capacity(renderables.len());

    for (entity, position, rotation, scale) in &renderables {
        let mut flags: u8 = CHANGED_TRANSFORM;

        let visible = world
            .get::<Visibility>(*entity)
            .map(|v| v.visible)
            .unwrap_or(true);

        let mesh_id = world.get::<MeshHandle>(*entity).map(|m| m.id).unwrap_or(0);

        let material_id = world
            .get::<MaterialHandle>(*entity)
            .map(|m| m.id)
            .unwrap_or(0);

        // Check optional component change flags via entity location.
        if let Some(loc) = world.entity_location(*entity) {
            let arch = world.archetypes().get(loc.archetype_id);
            let row = loc.row as usize;
            if arch
                .column::<Visibility>()
                .is_some_and(|c| c.changed_tick(row) > since_tick)
            {
                flags |= CHANGED_VISIBILITY;
            }
            if arch
                .column::<MeshHandle>()
                .is_some_and(|c| c.changed_tick(row) > since_tick)
            {
                flags |= CHANGED_MESH;
            }
            if arch
                .column::<MaterialHandle>()
                .is_some_and(|c| c.changed_tick(row) > since_tick)
            {
                flags |= CHANGED_MATERIAL;
            }
        }

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
        packet.change_flags.push(flags);
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
        assert_eq!(packet.transforms[0], 1.0);
        assert_eq!(packet.transforms[1], 2.0);
        assert_eq!(packet.transforms[2], 3.0);
        assert_eq!(packet.visibility[0], 1);
        assert_eq!(packet.mesh_handles[0], 10);
        assert_eq!(packet.material_handles[0], 20);
    }

    #[test]
    fn extract_defaults_for_missing_components() {
        let mut world = World::new();
        world.spawn((Transform::identity(),));

        let packet = extract_frame(&world);
        assert_eq!(packet.entity_count(), 1);
        assert_eq!(packet.visibility[0], 1);
        assert_eq!(packet.mesh_handles[0], 0);
        assert_eq!(packet.material_handles[0], 0);
    }

    #[test]
    fn extract_skips_entities_without_transform() {
        let mut world = World::new();
        world.spawn((Transform::identity(), MeshHandle { id: 1 }));
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
        assert_eq!(packet.visibility[0], 0);
    }

    #[test]
    fn extract_includes_entity_generation() {
        let mut world = World::new();
        let e = world.spawn((Transform::identity(),));
        let packet = extract_frame(&world);
        assert_eq!(packet.entity_generations[0], 0);

        world.despawn(e);
        world.spawn((Transform::identity(),));
        let packet = extract_frame(&world);
        assert_eq!(packet.entity_ids[0], 0);
        assert_eq!(packet.entity_generations[0], 1);
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
        assert!(ch.data[2].abs() < f32::EPSILON);
        assert!(ch.data[3].abs() < f32::EPSILON);
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

    #[test]
    fn incremental_extract_empty_when_nothing_changed() {
        let mut world = World::new();
        world.spawn((Transform::from_position(1.0, 0.0, 0.0),));
        let since = world.change_tick();
        world.advance_tick();

        let packet = extract_frame_incremental(&world, since);
        assert_eq!(packet.entity_count(), 0);
        assert!(packet.change_flags.is_empty());
    }

    #[test]
    fn incremental_extract_includes_changed_transform() {
        let mut world = World::new();
        let e = world.spawn((Transform::from_position(1.0, 0.0, 0.0),));
        let since = world.change_tick();
        world.advance_tick();

        world.get_mut::<Transform>(e).unwrap().position = [99.0, 0.0, 0.0];

        let packet = extract_frame_incremental(&world, since);
        assert_eq!(packet.entity_count(), 1);
        assert_eq!(packet.transforms[0], 99.0);
        assert!(packet.change_flags[0] & CHANGED_TRANSFORM != 0);
    }

    #[test]
    fn incremental_extract_skips_unchanged_entities() {
        let mut world = World::new();
        let e1 = world.spawn((Transform::from_position(1.0, 0.0, 0.0),));
        let _e2 = world.spawn((Transform::from_position(2.0, 0.0, 0.0),));
        let since = world.change_tick();
        world.advance_tick();

        world.get_mut::<Transform>(e1).unwrap().position[0] = 10.0;

        let packet = extract_frame_incremental(&world, since);
        assert_eq!(packet.entity_count(), 1);
        assert_eq!(packet.entity_ids[0], e1.index());
    }

    #[test]
    fn incremental_extract_flags_multiple_changes() {
        let mut world = World::new();
        let e = world.spawn((
            Transform::from_position(1.0, 0.0, 0.0),
            Visibility { visible: true },
            MaterialHandle { id: 1 },
        ));
        let since = world.change_tick();
        world.advance_tick();

        world.get_mut::<Transform>(e).unwrap().position[0] = 10.0;
        world.get_mut::<Visibility>(e).unwrap().visible = false;
        world.get_mut::<MaterialHandle>(e).unwrap().id = 99;

        let packet = extract_frame_incremental(&world, since);
        assert_eq!(packet.entity_count(), 1);
        let flags = packet.change_flags[0];
        assert!(flags & CHANGED_TRANSFORM != 0);
        assert!(flags & CHANGED_VISIBILITY != 0);
        assert!(flags & CHANGED_MATERIAL != 0);
    }

    #[test]
    fn incremental_extract_includes_newly_spawned() {
        let mut world = World::new();
        let since = world.change_tick();
        world.advance_tick();

        world.spawn((Transform::from_position(5.0, 0.0, 0.0),));

        let packet = extract_frame_incremental(&world, since);
        assert_eq!(packet.entity_count(), 1);
        assert_eq!(packet.transforms[0], 5.0);
    }
}
