// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

use galeon_engine::render::{MaterialHandle, MeshHandle, ParentEntity, Transform, Visibility};
use galeon_engine::{Entity, RenderChannelRegistry, World};

use crate::frame_packet::{
    CHANGED_MATERIAL, CHANGED_MESH, CHANGED_PARENT, CHANGED_TRANSFORM, CHANGED_VISIBILITY,
    ChannelData, FramePacket, SCENE_ROOT,
};

/// Extract render-facing data from the ECS world into a packed frame packet.
///
/// Single-pass query using optional components: iterates all entities with a
/// `Transform` component (the implicit "renderable" marker) and packs their
/// transform, visibility, mesh, material, and parent data into flat arrays
/// suitable for WASM transport.
///
/// Entities are depth-sorted so parents appear before their children. This
/// guarantees the JS-side scene graph can be built in a single forward pass.
///
/// If a [`RenderChannelRegistry`] resource is present, also extracts all
/// registered custom channels into `FramePacket::custom_channels`.
///
/// Missing optional components use sensible defaults:
/// - `Visibility`: defaults to visible (`true`)
/// - `MeshHandle`: defaults to `0` (no mesh)
/// - `MaterialHandle`: defaults to `0` (no material)
/// - `ParentEntity`: defaults to scene root ([`SCENE_ROOT`])
/// - Custom channels: defaults to `0.0` for all floats
type Renderable = (Entity, [f32; 3], [f32; 4], [f32; 3]);

/// Compute hierarchy depth for an entity by walking its parent chain.
/// Returns 0 for root entities, 1 for direct children, etc.
/// Caps at `max_depth` to guard against cycles.
fn hierarchy_depth(world: &World, entity: Entity, max_depth: u32) -> u32 {
    let mut depth = 0u32;
    let mut current = entity;
    while let Some(parent) = world.get::<ParentEntity>(current) {
        depth += 1;
        if depth >= max_depth {
            break;
        }
        current = parent.0;
    }
    depth
}

pub fn extract_frame(world: &World) -> FramePacket {
    let query = world.query::<(
        &Transform,
        Option<&Visibility>,
        Option<&MeshHandle>,
        Option<&MaterialHandle>,
    )>();

    // Collect into intermediate vec for depth sorting.
    struct Row {
        entity: Entity,
        transform: Transform,
        visible: bool,
        mesh_id: u32,
        material_id: u32,
        parent_id: u32,
        depth: u32,
    }

    let mut rows: Vec<Row> = query
        .map(|(entity, (transform, vis, mesh, mat))| {
            let parent_id = world
                .get::<ParentEntity>(entity)
                .map(|p| p.0.index())
                .unwrap_or(SCENE_ROOT);
            Row {
                entity,
                transform: *transform,
                visible: vis.map(|v| v.visible).unwrap_or(true),
                mesh_id: mesh.map(|m| m.id).unwrap_or(0),
                material_id: mat.map(|m| m.id).unwrap_or(0),
                parent_id,
                depth: hierarchy_depth(world, entity, 64),
            }
        })
        .collect();

    // Depth-sort: parents before children.
    rows.sort_by_key(|r| r.depth);

    let mut packet = FramePacket::with_capacity(rows.len());
    let mut entities = Vec::with_capacity(rows.len());

    for row in &rows {
        packet.push(
            row.entity.index(),
            row.entity.generation(),
            &row.transform.position,
            &row.transform.rotation,
            &row.transform.scale,
            row.visible,
            row.mesh_id,
            row.material_id,
            row.parent_id,
        );

        entities.push(row.entity);
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
/// The change flags further indicate if Visibility, MeshHandle,
/// MaterialHandle, or ParentEntity changed on those entities.
///
/// # Change detection precision
///
/// `QueryMut<T>` yields `Mut<T>` smart pointers that only stamp
/// `changed_tick` when written through (`DerefMut`). Systems that iterate
/// `QueryMut<Transform>` but only read some entities will not trigger
/// false positives — only actually-mutated entities appear in
/// `query_changed` results.
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

        let parent_id = world
            .get::<ParentEntity>(*entity)
            .map(|p| p.0.index())
            .unwrap_or(SCENE_ROOT);

        // Check optional component change flags via entity location.
        // SAFETY: `world` is borrowed immutably for this entire function, so no
        // archetype migration can occur. The row from `entity_location` is the
        // same row where `query_changed` found the entity.
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
            if arch
                .column::<ParentEntity>()
                .is_some_and(|c| c.changed_tick(row) > since_tick)
            {
                flags |= CHANGED_PARENT;
            }
        }

        packet.push_incremental(
            entity.index(),
            entity.generation(),
            position,
            rotation,
            scale,
            visible,
            mesh_id,
            material_id,
            parent_id,
            flags,
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

    // ---- Hierarchy tests ----

    #[test]
    fn extract_parent_id_defaults_to_scene_root() {
        let mut world = World::new();
        world.spawn((Transform::identity(),));

        let packet = extract_frame(&world);
        assert_eq!(packet.entity_count(), 1);
        assert_eq!(packet.parent_ids[0], SCENE_ROOT);
    }

    #[test]
    fn extract_parent_id_set_for_child() {
        let mut world = World::new();
        let parent = world.spawn((Transform::identity(),));
        world.spawn((Transform::from_position(1.0, 0.0, 0.0), ParentEntity(parent)));

        let packet = extract_frame(&world);
        assert_eq!(packet.entity_count(), 2);

        // Find the child in the packet.
        let child_idx = if packet.parent_ids[0] == SCENE_ROOT { 1 } else { 0 };
        assert_eq!(packet.parent_ids[child_idx], parent.index());
    }

    #[test]
    fn extract_depth_sorted_parent_before_child() {
        let mut world = World::new();
        // Spawn child first, then parent — extraction should still order parent first.
        let parent = world.spawn((Transform::identity(),));
        let _child = world.spawn((Transform::from_position(1.0, 0.0, 0.0), ParentEntity(parent)));

        let packet = extract_frame(&world);
        assert_eq!(packet.entity_count(), 2);

        // Parent (depth 0) should appear at index 0.
        assert_eq!(packet.parent_ids[0], SCENE_ROOT);
        assert_eq!(packet.entity_ids[0], parent.index());

        // Child (depth 1) should appear at index 1.
        assert_eq!(packet.parent_ids[1], parent.index());
    }

    #[test]
    fn extract_deep_hierarchy_ordering() {
        let mut world = World::new();
        let grandparent = world.spawn((Transform::identity(),));
        let parent = world.spawn((Transform::identity(), ParentEntity(grandparent)));
        let child = world.spawn((Transform::identity(), ParentEntity(parent)));

        let packet = extract_frame(&world);
        assert_eq!(packet.entity_count(), 3);

        // Verify depth ordering: grandparent, parent, child.
        assert_eq!(packet.entity_ids[0], grandparent.index());
        assert_eq!(packet.entity_ids[1], parent.index());
        assert_eq!(packet.entity_ids[2], child.index());

        assert_eq!(packet.parent_ids[0], SCENE_ROOT);
        assert_eq!(packet.parent_ids[1], grandparent.index());
        assert_eq!(packet.parent_ids[2], parent.index());
    }

    #[test]
    fn incremental_extract_includes_parent_id() {
        let mut world = World::new();
        let parent = world.spawn((Transform::identity(),));
        let child = world.spawn((Transform::identity(), ParentEntity(parent)));
        let since = world.change_tick();
        world.advance_tick();

        // Mutate the child's transform to trigger inclusion.
        world.get_mut::<Transform>(child).unwrap().position = [5.0, 0.0, 0.0];

        let packet = extract_frame_incremental(&world, since);
        assert_eq!(packet.entity_count(), 1);
        assert_eq!(packet.entity_ids[0], child.index());
        assert_eq!(packet.parent_ids[0], parent.index());
    }
}
