// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

use std::collections::HashSet;

use galeon_engine::render::{
    InstanceOf, MaterialHandle, MeshHandle, ObjectType, ParentEntity, Tint, Transform, Visibility,
};
use galeon_engine::{Entity, RenderChannelRegistry, RenderEventRegistry, World};

use crate::frame_packet::{
    CHANGED_INSTANCE_GROUP, CHANGED_MATERIAL, CHANGED_MESH, CHANGED_OBJECT_TYPE, CHANGED_PARENT,
    CHANGED_TINT, CHANGED_TRANSFORM, CHANGED_VISIBILITY, ChannelData, FramePacket,
    INSTANCE_GROUP_NONE, SCENE_ROOT,
};

/// Identity tint (white) — rendered untinted by the shader's color multiply.
const DEFAULT_TINT: [f32; 3] = [1.0, 1.0, 1.0];

fn resolved_tint(world: &World, entity: Entity) -> [f32; 3] {
    world
        .get::<Tint>(entity)
        .map(|t| t.0)
        .unwrap_or(DEFAULT_TINT)
}

/// Extract render-facing data from the ECS world into a packed frame packet.
///
/// # Render events
///
/// If a [`RenderEventRegistry`] is present, this function drains its
/// accumulation buffer into `FramePacket::events`. The buffer must have
/// been populated by prior [`World::flush_render_events`] calls —
/// [`Schedule::run`] does this automatically (once after deadlines, once
/// after systems). Calling `extract_frame` without a preceding flush
/// produces an empty event list.
///
/// # Entity extraction
///
/// Single-pass query using optional components: iterates all entities with a
/// `Transform` component (the implicit "renderable" marker) and packs their
/// transform, visibility, mesh, material, parent, and object-type data into
/// flat arrays suitable for WASM transport.
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
/// - `ObjectType`: defaults to mesh (`0`)
/// - Custom channels: defaults to `0.0` for all floats
type Renderable = (Entity, [f32; 3], [f32; 4], [f32; 3], u32, u8);

fn resolved_instance_group(world: &World, entity: Entity) -> u32 {
    world
        .get::<InstanceOf>(entity)
        .map(|i| i.0.id)
        .unwrap_or(INSTANCE_GROUP_NONE)
}

fn resolved_parent(world: &World, entity: Entity) -> Option<Entity> {
    let parent = world.get::<ParentEntity>(entity)?.0;
    if !world.is_alive(parent) || world.get::<Transform>(parent).is_none() {
        return None;
    }
    Some(parent)
}

fn resolved_parent_id(world: &World, entity: Entity) -> u32 {
    resolved_parent(world, entity)
        .map(|parent| parent.index())
        .unwrap_or(SCENE_ROOT)
}

/// Compute hierarchy depth for an entity by walking its parent chain.
/// Returns 0 for root entities, 1 for direct children, etc.
/// Caps at `max_depth` to guard against cycles.
fn hierarchy_depth(world: &World, entity: Entity, max_depth: u32) -> u32 {
    let mut depth = 0u32;
    let mut current = entity;
    while let Some(parent) = resolved_parent(world, current) {
        depth += 1;
        if depth >= max_depth {
            break;
        }
        current = parent;
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

    struct Row {
        entity: Entity,
        transform: Transform,
        visible: bool,
        mesh_id: u32,
        material_id: u32,
        parent_id: u32,
        object_type: u8,
        instance_group: u32,
        tint: [f32; 3],
        depth: u32,
    }

    let mut rows: Vec<Row> = query
        .map(|(entity, (transform, vis, mesh, mat))| Row {
            entity,
            transform: *transform,
            visible: vis.map(|v| v.visible).unwrap_or(true),
            mesh_id: mesh.map(|m| m.id).unwrap_or(0),
            material_id: mat.map(|m| m.id).unwrap_or(0),
            parent_id: resolved_parent_id(world, entity),
            object_type: world
                .get::<ObjectType>(entity)
                .map(|t| *t as u8)
                .unwrap_or(0),
            instance_group: resolved_instance_group(world, entity),
            tint: resolved_tint(world, entity),
            depth: hierarchy_depth(world, entity, 64),
        })
        .collect();

    rows.sort_by_key(|row| row.depth);

    let mut packet = FramePacket::with_capacity(rows.len());
    packet.frame_version = world.change_tick();
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
            row.object_type,
            row.instance_group,
            &row.tint,
        );
        entities.push(row.entity);
    }

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

    if let Some(registry) = world.try_resource::<RenderEventRegistry>() {
        packet.events = registry.drain();
    }

    packet
}

/// Incremental extraction: only entities whose renderable components changed
/// since `since_tick`. Each entity gets a `change_flags` bitmask indicating
/// which fields changed.
///
/// Entities are included when their `Transform` changed, when `ObjectType` or
/// `ParentEntity` changed (even if the transform did not), or when either
/// optional component was removed. Child entities whose configured parent is no
/// longer renderable are also emitted so the TS side can reparent them to the
/// scene root on the next frame.
///
/// # Change detection precision
///
/// `QueryMut<T>` yields `Mut<T>` smart pointers that only stamp
/// `changed_tick` when written through (`DerefMut`). Systems that iterate
/// `QueryMut<Transform>` but only read some entities will not trigger
/// false positives — only actually-mutated entities appear in
/// `query_changed` results.
pub fn extract_frame_incremental(world: &World, since_tick: u64) -> FramePacket {
    let mut seen: HashSet<Entity> = HashSet::new();
    let mut object_type_removed: HashSet<Entity> = HashSet::new();
    let mut parent_removed: HashSet<Entity> = HashSet::new();
    let mut instance_group_removed: HashSet<Entity> = HashSet::new();
    let mut tint_removed: HashSet<Entity> = HashSet::new();
    let mut parents_with_new_transform: HashSet<Entity> = HashSet::new();
    let mut renderables: Vec<Renderable> = Vec::new();

    for (entity, _) in world.query_added::<Transform>(since_tick) {
        parents_with_new_transform.insert(entity);
    }

    for (entity, transform) in world.query_changed::<Transform>(since_tick) {
        let object_type = world
            .get::<ObjectType>(entity)
            .map(|o| *o as u8)
            .unwrap_or(0);
        renderables.push((
            entity,
            transform.position,
            transform.rotation,
            transform.scale,
            resolved_parent_id(world, entity),
            object_type,
        ));
        seen.insert(entity);
    }

    for (entity, _) in world.query_changed::<ObjectType>(since_tick) {
        if seen.contains(&entity) {
            continue;
        }
        let Some(transform) = world.get::<Transform>(entity) else {
            continue;
        };
        let object_type = world
            .get::<ObjectType>(entity)
            .map(|o| *o as u8)
            .unwrap_or(0);
        renderables.push((
            entity,
            transform.position,
            transform.rotation,
            transform.scale,
            resolved_parent_id(world, entity),
            object_type,
        ));
        seen.insert(entity);
    }

    for entity in world.component_removals_since::<ObjectType>(since_tick) {
        object_type_removed.insert(entity);
        if seen.contains(&entity) {
            continue;
        }
        let Some(transform) = world.get::<Transform>(entity) else {
            continue;
        };
        let object_type = world
            .get::<ObjectType>(entity)
            .map(|o| *o as u8)
            .unwrap_or(0);
        renderables.push((
            entity,
            transform.position,
            transform.rotation,
            transform.scale,
            resolved_parent_id(world, entity),
            object_type,
        ));
        seen.insert(entity);
    }

    for (entity, _) in world.query_changed::<ParentEntity>(since_tick) {
        if seen.contains(&entity) {
            continue;
        }
        let Some(transform) = world.get::<Transform>(entity) else {
            continue;
        };
        let object_type = world
            .get::<ObjectType>(entity)
            .map(|o| *o as u8)
            .unwrap_or(0);
        renderables.push((
            entity,
            transform.position,
            transform.rotation,
            transform.scale,
            resolved_parent_id(world, entity),
            object_type,
        ));
        seen.insert(entity);
    }

    for entity in world.component_removals_since::<ParentEntity>(since_tick) {
        parent_removed.insert(entity);
        if seen.contains(&entity) {
            continue;
        }
        let Some(transform) = world.get::<Transform>(entity) else {
            continue;
        };
        let object_type = world
            .get::<ObjectType>(entity)
            .map(|o| *o as u8)
            .unwrap_or(0);
        renderables.push((
            entity,
            transform.position,
            transform.rotation,
            transform.scale,
            resolved_parent_id(world, entity),
            object_type,
        ));
        seen.insert(entity);
    }

    for (entity, _) in world.query_changed::<InstanceOf>(since_tick) {
        if seen.contains(&entity) {
            continue;
        }
        let Some(transform) = world.get::<Transform>(entity) else {
            continue;
        };
        let object_type = world
            .get::<ObjectType>(entity)
            .map(|o| *o as u8)
            .unwrap_or(0);
        renderables.push((
            entity,
            transform.position,
            transform.rotation,
            transform.scale,
            resolved_parent_id(world, entity),
            object_type,
        ));
        seen.insert(entity);
    }

    for entity in world.component_removals_since::<InstanceOf>(since_tick) {
        instance_group_removed.insert(entity);
        if seen.contains(&entity) {
            continue;
        }
        let Some(transform) = world.get::<Transform>(entity) else {
            continue;
        };
        let object_type = world
            .get::<ObjectType>(entity)
            .map(|o| *o as u8)
            .unwrap_or(0);
        renderables.push((
            entity,
            transform.position,
            transform.rotation,
            transform.scale,
            resolved_parent_id(world, entity),
            object_type,
        ));
        seen.insert(entity);
    }

    for (entity, _) in world.query_changed::<Tint>(since_tick) {
        if seen.contains(&entity) {
            continue;
        }
        let Some(transform) = world.get::<Transform>(entity) else {
            continue;
        };
        let object_type = world
            .get::<ObjectType>(entity)
            .map(|o| *o as u8)
            .unwrap_or(0);
        renderables.push((
            entity,
            transform.position,
            transform.rotation,
            transform.scale,
            resolved_parent_id(world, entity),
            object_type,
        ));
        seen.insert(entity);
    }

    for entity in world.component_removals_since::<Tint>(since_tick) {
        tint_removed.insert(entity);
        if seen.contains(&entity) {
            continue;
        }
        let Some(transform) = world.get::<Transform>(entity) else {
            continue;
        };
        let object_type = world
            .get::<ObjectType>(entity)
            .map(|o| *o as u8)
            .unwrap_or(0);
        renderables.push((
            entity,
            transform.position,
            transform.rotation,
            transform.scale,
            resolved_parent_id(world, entity),
            object_type,
        ));
        seen.insert(entity);
    }

    for (entity, (transform, parent)) in world.query::<(&Transform, &ParentEntity)>() {
        if seen.contains(&entity) {
            continue;
        }
        let parent_entity = parent.0;
        let parent_missing =
            !world.is_alive(parent_entity) || world.get::<Transform>(parent_entity).is_none();
        let parent_became_renderable = parents_with_new_transform.contains(&parent_entity);
        if !parent_missing && !parent_became_renderable {
            continue;
        }
        let object_type = world
            .get::<ObjectType>(entity)
            .map(|o| *o as u8)
            .unwrap_or(0);
        renderables.push((
            entity,
            transform.position,
            transform.rotation,
            transform.scale,
            resolved_parent_id(world, entity),
            object_type,
        ));
        seen.insert(entity);
    }

    renderables.sort_by_key(|(entity, ..)| hierarchy_depth(world, *entity, 64));

    let mut packet = FramePacket::with_capacity(renderables.len());
    packet.frame_version = world.change_tick();

    for (entity, position, rotation, scale, parent_id, object_type) in &renderables {
        let mut flags: u8 = 0;

        let visible = world
            .get::<Visibility>(*entity)
            .map(|v| v.visible)
            .unwrap_or(true);
        let mesh_id = world.get::<MeshHandle>(*entity).map(|m| m.id).unwrap_or(0);
        let material_id = world
            .get::<MaterialHandle>(*entity)
            .map(|m| m.id)
            .unwrap_or(0);

        if let Some(loc) = world.entity_location(*entity) {
            let arch = world.archetypes().get(loc.archetype_id);
            let row = loc.row as usize;
            if arch
                .column::<Transform>()
                .is_some_and(|column| column.changed_tick(row) > since_tick)
            {
                flags |= CHANGED_TRANSFORM;
            }
            if arch
                .column::<Visibility>()
                .is_some_and(|column| column.changed_tick(row) > since_tick)
            {
                flags |= CHANGED_VISIBILITY;
            }
            if arch
                .column::<MeshHandle>()
                .is_some_and(|column| column.changed_tick(row) > since_tick)
            {
                flags |= CHANGED_MESH;
            }
            if arch
                .column::<MaterialHandle>()
                .is_some_and(|column| column.changed_tick(row) > since_tick)
            {
                flags |= CHANGED_MATERIAL;
            }
            if object_type_removed.contains(entity)
                || arch
                    .column::<ObjectType>()
                    .is_some_and(|column| column.changed_tick(row) > since_tick)
            {
                flags |= CHANGED_OBJECT_TYPE;
            }
            if parent_removed.contains(entity)
                || arch.column::<ParentEntity>().is_some_and(|column| {
                    column.changed_tick(row) > since_tick || *parent_id == SCENE_ROOT
                })
                || (arch.column::<ParentEntity>().is_none()
                    && parents_with_new_transform.contains(entity))
            {
                flags |= CHANGED_PARENT;
            }
            if instance_group_removed.contains(entity)
                || arch
                    .column::<InstanceOf>()
                    .is_some_and(|column| column.changed_tick(row) > since_tick)
            {
                flags |= CHANGED_INSTANCE_GROUP;
            }
            if tint_removed.contains(entity)
                || arch
                    .column::<Tint>()
                    .is_some_and(|column| column.changed_tick(row) > since_tick)
            {
                flags |= CHANGED_TINT;
            }
        }

        let instance_group = resolved_instance_group(world, *entity);
        let tint = resolved_tint(world, *entity);

        packet.push_incremental(
            entity.index(),
            entity.generation(),
            position,
            rotation,
            scale,
            visible,
            mesh_id,
            material_id,
            *parent_id,
            *object_type,
            instance_group,
            &tint,
            flags,
        );
    }

    // Events are always fully extracted (not incremental — they are ephemeral).
    if let Some(registry) = world.try_resource::<RenderEventRegistry>() {
        packet.events = registry.drain();
    }

    packet
}

#[cfg(test)]
mod tests {
    use super::*;
    use galeon_engine::render::{
        MaterialHandle, MeshHandle, ObjectType, ParentEntity, Transform, Visibility,
    };

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
        assert_eq!(packet.parent_ids[0], SCENE_ROOT);
        assert_eq!(packet.object_types[0], 0);
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
        let entity = world.spawn((Transform::identity(),));
        let packet = extract_frame(&world);
        assert_eq!(packet.entity_generations[0], 0);

        world.despawn(entity);
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
        let mut registry = galeon_engine::RenderChannelRegistry::new();
        registry.register::<WearState>("wear");
        world.insert_resource(registry);

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

        let channel = packet.channel("wear").unwrap();
        assert_eq!(channel.stride, 2);
        assert_eq!(channel.data.len(), 4);
        assert!((channel.data[0] - 0.5).abs() < f32::EPSILON);
        assert!((channel.data[1] - 0.3).abs() < f32::EPSILON);
        assert!((channel.data[2] - 0.8).abs() < f32::EPSILON);
        assert!((channel.data[3] - 0.1).abs() < f32::EPSILON);
    }

    #[test]
    fn extract_channel_zeroes_when_component_absent() {
        let mut world = World::new();
        let mut registry = galeon_engine::RenderChannelRegistry::new();
        registry.register::<WearState>("wear");
        world.insert_resource(registry);

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

        let channel = packet.channel("wear").unwrap();
        assert!((channel.data[0] - 0.5).abs() < f32::EPSILON);
        assert!((channel.data[1] - 0.3).abs() < f32::EPSILON);
        assert!(channel.data[2].abs() < f32::EPSILON);
        assert!(channel.data[3].abs() < f32::EPSILON);
    }

    #[test]
    fn extract_empty_channel_when_no_entities() {
        let mut world = World::new();
        let mut registry = galeon_engine::RenderChannelRegistry::new();
        registry.register::<WearState>("wear");
        world.insert_resource(registry);

        let packet = extract_frame(&world);
        assert_eq!(packet.entity_count(), 0);
        let channel = packet.channel("wear").unwrap();
        assert!(channel.data.is_empty());
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
        let entity = world.spawn((Transform::from_position(1.0, 0.0, 0.0),));
        let since = world.change_tick();
        world.advance_tick();

        world.get_mut::<Transform>(entity).unwrap().position = [99.0, 0.0, 0.0];

        let packet = extract_frame_incremental(&world, since);
        assert_eq!(packet.entity_count(), 1);
        assert_eq!(packet.transforms[0], 99.0);
        assert!(packet.change_flags[0] & CHANGED_TRANSFORM != 0);
    }

    #[test]
    fn incremental_extract_skips_unchanged_entities() {
        let mut world = World::new();
        let changed = world.spawn((Transform::from_position(1.0, 0.0, 0.0),));
        let _unchanged = world.spawn((Transform::from_position(2.0, 0.0, 0.0),));
        let since = world.change_tick();
        world.advance_tick();

        world.get_mut::<Transform>(changed).unwrap().position[0] = 10.0;

        let packet = extract_frame_incremental(&world, since);
        assert_eq!(packet.entity_count(), 1);
        assert_eq!(packet.entity_ids[0], changed.index());
    }

    #[test]
    fn incremental_extract_flags_multiple_changes() {
        let mut world = World::new();
        let entity = world.spawn((
            Transform::from_position(1.0, 0.0, 0.0),
            Visibility { visible: true },
            MaterialHandle { id: 1 },
        ));
        let since = world.change_tick();
        world.advance_tick();

        world.get_mut::<Transform>(entity).unwrap().position[0] = 10.0;
        world.get_mut::<Visibility>(entity).unwrap().visible = false;
        world.get_mut::<MaterialHandle>(entity).unwrap().id = 99;

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

    #[test]
    fn incremental_extract_includes_object_type_change_without_transform() {
        let mut world = World::new();
        let entity = world.spawn((Transform::from_position(1.0, 0.0, 0.0), ObjectType::Mesh));
        let since = world.change_tick();
        world.advance_tick();

        *world.get_mut::<ObjectType>(entity).unwrap() = ObjectType::PointLight;

        let packet = extract_frame_incremental(&world, since);
        assert_eq!(packet.entity_count(), 1);
        assert_eq!(packet.entity_ids[0], entity.index());
        assert_eq!(packet.object_types[0], ObjectType::PointLight as u8);
        let flags = packet.change_flags[0];
        assert!(flags & CHANGED_OBJECT_TYPE != 0);
        assert!(flags & CHANGED_TRANSFORM == 0);
    }

    #[test]
    fn incremental_extract_includes_object_type_removal_without_transform() {
        let mut world = World::new();
        let entity = world.spawn((
            Transform::from_position(1.0, 0.0, 0.0),
            ObjectType::PointLight,
        ));
        let since = world.change_tick();
        world.advance_tick();

        world.remove::<ObjectType>(entity);

        let packet = extract_frame_incremental(&world, since);
        assert_eq!(packet.entity_count(), 1);
        assert_eq!(packet.entity_ids[0], entity.index());
        assert_eq!(packet.object_types[0], ObjectType::Mesh as u8);
        let flags = packet.change_flags[0];
        assert!(flags & CHANGED_OBJECT_TYPE != 0);
        assert!(flags & CHANGED_TRANSFORM == 0);
    }

    #[test]
    fn extract_object_type_component() {
        let mut world = World::new();
        world.spawn((
            Transform::from_position(1.0, 0.0, 0.0),
            ObjectType::PointLight,
        ));
        world.spawn((Transform::from_position(2.0, 0.0, 0.0), ObjectType::Mesh));
        world.spawn((Transform::from_position(3.0, 0.0, 0.0),));

        let packet = extract_frame(&world);
        assert_eq!(packet.entity_count(), 3);

        for i in 0..3 {
            let pos_x = packet.transforms[i * 10];
            match pos_x as u32 {
                1 => assert_eq!(packet.object_types[i], ObjectType::PointLight as u8),
                2 => assert_eq!(packet.object_types[i], ObjectType::Mesh as u8),
                3 => assert_eq!(packet.object_types[i], 0),
                _ => panic!("unexpected position"),
            }
        }
    }

    #[test]
    fn extract_object_type_defaults_to_mesh() {
        let mut world = World::new();
        world.spawn((Transform::identity(),));

        let packet = extract_frame(&world);
        assert_eq!(packet.object_types[0], 0);
    }

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
        world.spawn((
            Transform::from_position(1.0, 0.0, 0.0),
            ParentEntity(parent),
        ));

        let packet = extract_frame(&world);
        assert_eq!(packet.entity_count(), 2);

        let child_index = if packet.parent_ids[0] == SCENE_ROOT {
            1
        } else {
            0
        };
        assert_eq!(packet.parent_ids[child_index], parent.index());
    }

    #[test]
    fn extract_depth_sorted_parent_before_child() {
        let mut world = World::new();
        let parent = world.spawn((Transform::identity(),));
        let _child = world.spawn((
            Transform::from_position(1.0, 0.0, 0.0),
            ParentEntity(parent),
        ));

        let packet = extract_frame(&world);
        assert_eq!(packet.entity_count(), 2);
        assert_eq!(packet.parent_ids[0], SCENE_ROOT);
        assert_eq!(packet.entity_ids[0], parent.index());
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
        assert_eq!(packet.entity_ids[0], grandparent.index());
        assert_eq!(packet.entity_ids[1], parent.index());
        assert_eq!(packet.entity_ids[2], child.index());
        assert_eq!(packet.parent_ids[0], SCENE_ROOT);
        assert_eq!(packet.parent_ids[1], grandparent.index());
        assert_eq!(packet.parent_ids[2], parent.index());
    }

    #[test]
    fn extract_dead_parent_defaults_child_to_scene_root() {
        let mut world = World::new();
        let parent = world.spawn((Transform::identity(),));
        let child = world.spawn((Transform::identity(), ParentEntity(parent)));
        world.despawn(parent);

        let packet = extract_frame(&world);
        assert_eq!(packet.entity_count(), 1);
        assert_eq!(packet.entity_ids[0], child.index());
        assert_eq!(packet.parent_ids[0], SCENE_ROOT);
    }

    #[test]
    fn incremental_extract_includes_parent_id_when_transform_changes() {
        let mut world = World::new();
        let parent = world.spawn((Transform::identity(),));
        let child = world.spawn((Transform::identity(), ParentEntity(parent)));
        let since = world.change_tick();
        world.advance_tick();

        world.get_mut::<Transform>(child).unwrap().position = [5.0, 0.0, 0.0];

        let packet = extract_frame_incremental(&world, since);
        assert_eq!(packet.entity_count(), 1);
        assert_eq!(packet.entity_ids[0], child.index());
        assert_eq!(packet.parent_ids[0], parent.index());
    }

    #[test]
    fn incremental_extract_includes_parent_change_without_transform() {
        let mut world = World::new();
        let parent_a = world.spawn((Transform::identity(),));
        let parent_b = world.spawn((Transform::from_position(2.0, 0.0, 0.0),));
        let child = world.spawn((Transform::identity(), ParentEntity(parent_a)));
        let since = world.change_tick();
        world.advance_tick();

        *world.get_mut::<ParentEntity>(child).unwrap() = ParentEntity(parent_b);

        let packet = extract_frame_incremental(&world, since);
        assert_eq!(packet.entity_count(), 1);
        assert_eq!(packet.entity_ids[0], child.index());
        assert_eq!(packet.parent_ids[0], parent_b.index());
        let flags = packet.change_flags[0];
        assert!(flags & CHANGED_PARENT != 0);
        assert!(flags & CHANGED_TRANSFORM == 0);
    }

    #[test]
    fn incremental_extract_includes_parent_removal_without_transform() {
        let mut world = World::new();
        let parent = world.spawn((Transform::identity(),));
        let child = world.spawn((Transform::identity(), ParentEntity(parent)));
        let since = world.change_tick();
        world.advance_tick();

        world.remove::<ParentEntity>(child);

        let packet = extract_frame_incremental(&world, since);
        assert_eq!(packet.entity_count(), 1);
        assert_eq!(packet.entity_ids[0], child.index());
        assert_eq!(packet.parent_ids[0], SCENE_ROOT);
        let flags = packet.change_flags[0];
        assert!(flags & CHANGED_PARENT != 0);
        assert!(flags & CHANGED_TRANSFORM == 0);
    }

    #[test]
    fn incremental_extract_no_parent_flag_for_existing_root_entity() {
        let mut world = World::new();
        let entity = world.spawn((Transform::from_position(1.0, 0.0, 0.0),));
        let since = world.change_tick();
        world.advance_tick();

        world.get_mut::<Transform>(entity).unwrap().position = [99.0, 0.0, 0.0];

        let packet = extract_frame_incremental(&world, since);
        assert_eq!(packet.entity_count(), 1);
        assert!(packet.change_flags[0] & CHANGED_TRANSFORM != 0);
        assert!(packet.change_flags[0] & CHANGED_PARENT == 0);
    }

    #[test]
    fn incremental_extract_newly_spawned_root_gets_parent_flag() {
        let mut world = World::new();
        let since = world.change_tick();
        world.advance_tick();

        world.spawn((Transform::from_position(5.0, 0.0, 0.0),));

        let packet = extract_frame_incremental(&world, since);
        assert_eq!(packet.entity_count(), 1);
        assert!(packet.change_flags[0] & CHANGED_PARENT != 0);
    }

    #[test]
    fn extract_frame_sets_frame_version_to_change_tick() {
        let mut world = World::new();
        world.spawn((Transform::identity(),));
        let packet = extract_frame(&world);
        assert_eq!(packet.frame_version, world.change_tick());
    }

    #[test]
    fn incremental_extract_sets_frame_version_to_change_tick() {
        let mut world = World::new();
        let e = world.spawn((Transform::from_position(1.0, 0.0, 0.0),));
        let since = world.change_tick();
        world.advance_tick();
        world.get_mut::<Transform>(e).unwrap().position[0] = 2.0;

        let packet = extract_frame_incremental(&world, since);
        assert_eq!(packet.frame_version, world.change_tick());
    }

    #[test]
    fn frame_version_changes_after_advance_tick() {
        let mut world = World::new();
        world.spawn((Transform::identity(),));

        let v1 = extract_frame(&world).frame_version;
        world.advance_tick();
        let v2 = extract_frame(&world).frame_version;
        assert!(v2 > v1);
    }

    // -------------------------------------------------------------------------
    // Render event extraction integration tests
    // -------------------------------------------------------------------------

    #[derive(Debug)]
    struct TestImpact {
        entity_index: u32,
        pos: [f32; 3],
        force: f32,
    }

    impl galeon_engine::RenderEvent for TestImpact {
        const KIND: u32 = 1;
        fn entity(&self) -> u32 {
            self.entity_index
        }
        fn position(&self) -> [f32; 3] {
            self.pos
        }
        fn intensity(&self) -> f32 {
            self.force
        }
    }

    #[test]
    fn extract_no_events_when_registry_absent() {
        let mut world = World::new();
        world.spawn((Transform::identity(),));
        let packet = extract_frame(&world);
        assert_eq!(packet.event_count(), 0);
    }

    #[test]
    fn extract_no_events_when_none_sent() {
        let mut world = World::new();
        world.add_event::<TestImpact>();
        let mut registry = galeon_engine::RenderEventRegistry::new();
        registry.register::<TestImpact>();
        world.insert_resource(registry);

        world.spawn((Transform::identity(),));
        world.flush_render_events();
        let packet = extract_frame(&world);
        assert_eq!(packet.event_count(), 0);
    }

    #[test]
    fn extract_events_in_full_extraction() {
        let mut world = World::new();
        world.add_event::<TestImpact>();

        let mut registry = galeon_engine::RenderEventRegistry::new();
        registry.register::<TestImpact>();
        world.insert_resource(registry);

        world
            .resource_mut::<galeon_engine::Events<TestImpact>>()
            .send(TestImpact {
                entity_index: 42,
                pos: [1.0, 2.0, 3.0],
                force: 0.75,
            });

        world.spawn((Transform::identity(),));
        // Simulate schedule: flush_render_events() after systems.
        world.flush_render_events();
        let packet = extract_frame(&world);
        assert_eq!(packet.event_count(), 1);
        assert_eq!(packet.events[0].kind, 1);
        assert_eq!(packet.events[0].entity, 42);
        assert_eq!(packet.events[0].position, [1.0, 2.0, 3.0]);
        assert!((packet.events[0].intensity - 0.75).abs() < f32::EPSILON);
    }

    #[test]
    fn extract_events_in_incremental_extraction() {
        let mut world = World::new();
        world.add_event::<TestImpact>();

        let mut registry = galeon_engine::RenderEventRegistry::new();
        registry.register::<TestImpact>();
        world.insert_resource(registry);

        world.spawn((Transform::from_position(1.0, 0.0, 0.0),));
        let since = world.change_tick();
        world.advance_tick();

        world
            .resource_mut::<galeon_engine::Events<TestImpact>>()
            .send(TestImpact {
                entity_index: 7,
                pos: [5.0, 5.0, 5.0],
                force: 3.0,
            });

        world.flush_render_events();
        let packet = extract_frame_incremental(&world, since);
        assert_eq!(packet.event_count(), 1);
        assert_eq!(packet.events[0].kind, 1);
        assert_eq!(packet.events[0].entity, 7);
    }

    #[test]
    fn extract_multiple_events_same_frame() {
        let mut world = World::new();
        world.add_event::<TestImpact>();

        let mut registry = galeon_engine::RenderEventRegistry::new();
        registry.register::<TestImpact>();
        world.insert_resource(registry);

        let events = world.resource_mut::<galeon_engine::Events<TestImpact>>();
        events.send(TestImpact {
            entity_index: 1,
            pos: [0.0; 3],
            force: 1.0,
        });
        events.send(TestImpact {
            entity_index: 2,
            pos: [10.0; 3],
            force: 0.5,
        });

        world.spawn((Transform::identity(),));
        world.flush_render_events();
        let packet = extract_frame(&world);
        assert_eq!(packet.event_count(), 2);
    }

    #[test]
    fn multi_tick_events_accumulate() {
        let mut world = World::new();
        world.add_event::<TestImpact>();

        let mut registry = galeon_engine::RenderEventRegistry::new();
        registry.register::<TestImpact>();
        world.insert_resource(registry);
        world.spawn((Transform::identity(),));

        // Tick 1: send event, flush, then swap (simulating schedule flow).
        world
            .resource_mut::<galeon_engine::Events<TestImpact>>()
            .send(TestImpact {
                entity_index: 1,
                pos: [0.0; 3],
                force: 1.0,
            });
        world.flush_render_events();
        world.update_events();

        // Tick 2: send another event, flush, swap.
        world
            .resource_mut::<galeon_engine::Events<TestImpact>>()
            .send(TestImpact {
                entity_index: 2,
                pos: [5.0; 3],
                force: 0.5,
            });
        world.flush_render_events();
        world.update_events();

        // Extraction after both ticks: both events present.
        let packet = extract_frame(&world);
        assert_eq!(packet.event_count(), 2);
        assert_eq!(packet.events[0].entity, 1);
        assert_eq!(packet.events[1].entity, 2);
    }

    // -------------------------------------------------------------------------
    // InstanceOf / instance_groups extraction (issue #215 T1)
    // -------------------------------------------------------------------------

    #[test]
    fn extract_populates_instance_group_for_tagged_entity() {
        use galeon_engine::render::InstanceOf;

        let mut world = World::new();
        world.spawn((
            Transform::identity(),
            MeshHandle { id: 7 },
            InstanceOf(MeshHandle { id: 7 }),
        ));

        let packet = extract_frame(&world);
        assert_eq!(packet.entity_count(), 1);
        assert_eq!(packet.instance_groups[0], 7);
    }

    #[test]
    fn extract_instance_group_none_for_untagged_entity() {
        use crate::frame_packet::INSTANCE_GROUP_NONE;

        let mut world = World::new();
        world.spawn((Transform::identity(), MeshHandle { id: 7 }));

        let packet = extract_frame(&world);
        assert_eq!(packet.entity_count(), 1);
        assert_eq!(packet.instance_groups[0], INSTANCE_GROUP_NONE);
    }

    #[test]
    fn extract_instance_group_for_mixed_scene() {
        use crate::frame_packet::INSTANCE_GROUP_NONE;
        use galeon_engine::render::InstanceOf;

        let mut world = World::new();
        let tagged = world.spawn((
            Transform::from_position(1.0, 0.0, 0.0),
            MeshHandle { id: 5 },
            InstanceOf(MeshHandle { id: 5 }),
        ));
        let untagged = world.spawn((Transform::from_position(2.0, 0.0, 0.0),));

        let packet = extract_frame(&world);
        assert_eq!(packet.entity_count(), 2);

        for i in 0..2 {
            let entity_id = packet.entity_ids[i];
            if entity_id == tagged.index() {
                assert_eq!(packet.instance_groups[i], 5);
            } else if entity_id == untagged.index() {
                assert_eq!(packet.instance_groups[i], INSTANCE_GROUP_NONE);
            } else {
                panic!("unexpected entity id {entity_id}");
            }
        }
    }

    #[test]
    fn incremental_extract_populates_instance_group_for_tagged_entity() {
        use galeon_engine::render::InstanceOf;

        let mut world = World::new();
        let since = world.change_tick();
        world.advance_tick();

        world.spawn((
            Transform::from_position(1.0, 0.0, 0.0),
            MeshHandle { id: 9 },
            InstanceOf(MeshHandle { id: 9 }),
        ));

        let packet = extract_frame_incremental(&world, since);
        assert_eq!(packet.entity_count(), 1);
        assert_eq!(packet.instance_groups[0], 9);
    }

    #[test]
    fn incremental_extract_instance_group_none_for_untagged_entity() {
        use crate::frame_packet::INSTANCE_GROUP_NONE;

        let mut world = World::new();
        let since = world.change_tick();
        world.advance_tick();

        world.spawn((Transform::from_position(1.0, 0.0, 0.0),));

        let packet = extract_frame_incremental(&world, since);
        assert_eq!(packet.entity_count(), 1);
        assert_eq!(packet.instance_groups[0], INSTANCE_GROUP_NONE);
    }

    #[test]
    fn incremental_extract_flags_instance_group_added() {
        use crate::frame_packet::CHANGED_INSTANCE_GROUP;
        use galeon_engine::render::InstanceOf;

        let mut world = World::new();
        let entity = world.spawn((Transform::from_position(1.0, 0.0, 0.0),));
        let since = world.change_tick();
        world.advance_tick();

        world.insert(entity, InstanceOf(MeshHandle { id: 5 }));

        let packet = extract_frame_incremental(&world, since);
        assert_eq!(packet.entity_count(), 1);
        assert_eq!(packet.entity_ids[0], entity.index());
        assert_eq!(packet.instance_groups[0], 5);
        let flags = packet.change_flags[0];
        assert!(
            flags & CHANGED_INSTANCE_GROUP != 0,
            "expected CHANGED_INSTANCE_GROUP, got {flags:#b}"
        );
        assert!(flags & CHANGED_TRANSFORM == 0);
    }

    #[test]
    fn incremental_extract_flags_instance_group_removed() {
        use crate::frame_packet::{CHANGED_INSTANCE_GROUP, INSTANCE_GROUP_NONE};
        use galeon_engine::render::InstanceOf;

        let mut world = World::new();
        let entity = world.spawn((
            Transform::from_position(1.0, 0.0, 0.0),
            InstanceOf(MeshHandle { id: 5 }),
        ));
        let since = world.change_tick();
        world.advance_tick();

        world.remove::<InstanceOf>(entity);

        let packet = extract_frame_incremental(&world, since);
        assert_eq!(packet.entity_count(), 1);
        assert_eq!(packet.entity_ids[0], entity.index());
        assert_eq!(packet.instance_groups[0], INSTANCE_GROUP_NONE);
        let flags = packet.change_flags[0];
        assert!(
            flags & CHANGED_INSTANCE_GROUP != 0,
            "expected CHANGED_INSTANCE_GROUP, got {flags:#b}"
        );
        assert!(flags & CHANGED_TRANSFORM == 0);
    }

    #[test]
    fn incremental_extract_no_instance_group_flag_when_unchanged() {
        use crate::frame_packet::CHANGED_INSTANCE_GROUP;
        use galeon_engine::render::InstanceOf;

        let mut world = World::new();
        let entity = world.spawn((
            Transform::from_position(1.0, 0.0, 0.0),
            InstanceOf(MeshHandle { id: 5 }),
        ));
        let since = world.change_tick();
        world.advance_tick();

        world.get_mut::<Transform>(entity).unwrap().position[0] = 99.0;

        let packet = extract_frame_incremental(&world, since);
        assert_eq!(packet.entity_count(), 1);
        let flags = packet.change_flags[0];
        assert!(flags & CHANGED_TRANSFORM != 0);
        assert!(flags & CHANGED_INSTANCE_GROUP == 0);
    }

    // -------------------------------------------------------------------------
    // Tint / tints extraction (issue #215 T3)
    // -------------------------------------------------------------------------

    #[test]
    fn extract_populates_tint_for_tagged_entity() {
        use galeon_engine::render::Tint;

        let mut world = World::new();
        world.spawn((Transform::identity(), Tint([0.25, 0.5, 1.0])));

        let packet = extract_frame(&world);
        assert_eq!(packet.entity_count(), 1);
        assert_eq!(packet.tints.len(), 3);
        assert!((packet.tints[0] - 0.25).abs() < f32::EPSILON);
        assert!((packet.tints[1] - 0.5).abs() < f32::EPSILON);
        assert!((packet.tints[2] - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn extract_tint_default_white_for_untagged_entity() {
        let mut world = World::new();
        world.spawn((Transform::identity(),));

        let packet = extract_frame(&world);
        assert_eq!(packet.entity_count(), 1);
        assert_eq!(packet.tints, vec![1.0, 1.0, 1.0]);
    }

    #[test]
    fn extract_tint_for_mixed_scene() {
        use galeon_engine::render::Tint;

        let mut world = World::new();
        world.spawn((Transform::identity(), Tint([1.0, 0.0, 0.0])));
        world.spawn((Transform::identity(),));
        world.spawn((Transform::identity(), Tint([0.0, 1.0, 0.0])));

        let packet = extract_frame(&world);
        assert_eq!(packet.entity_count(), 3);
        assert_eq!(packet.tints.len(), 9);
        // Order matches entity_ids; we only assert the multiset of triples.
        let triples: Vec<[f32; 3]> = packet
            .tints
            .chunks_exact(3)
            .map(|c| [c[0], c[1], c[2]])
            .collect();
        assert!(triples.contains(&[1.0, 0.0, 0.0]));
        assert!(triples.contains(&[0.0, 1.0, 0.0]));
        assert!(triples.contains(&[1.0, 1.0, 1.0]));
    }

    #[test]
    fn incremental_extract_flags_tint_added() {
        use crate::frame_packet::CHANGED_TINT;
        use galeon_engine::render::Tint;

        let mut world = World::new();
        let entity = world.spawn((Transform::identity(),));
        let since = world.change_tick();
        world.advance_tick();

        world.insert(entity, Tint([0.0, 0.5, 1.0]));

        let packet = extract_frame_incremental(&world, since);
        assert_eq!(packet.entity_count(), 1);
        assert!((packet.tints[0] - 0.0).abs() < f32::EPSILON);
        assert!((packet.tints[1] - 0.5).abs() < f32::EPSILON);
        assert!((packet.tints[2] - 1.0).abs() < f32::EPSILON);
        let flags = packet.change_flags[0];
        assert!(
            flags & CHANGED_TINT != 0,
            "expected CHANGED_TINT, got {flags:#b}"
        );
    }

    #[test]
    fn incremental_extract_flags_tint_removed() {
        use crate::frame_packet::CHANGED_TINT;
        use galeon_engine::render::Tint;

        let mut world = World::new();
        let entity = world.spawn((Transform::identity(), Tint([0.5, 0.5, 0.5])));
        let since = world.change_tick();
        world.advance_tick();

        world.remove::<Tint>(entity);

        let packet = extract_frame_incremental(&world, since);
        assert_eq!(packet.entity_count(), 1);
        // After removal the entity falls back to the white default.
        assert_eq!(packet.tints, vec![1.0, 1.0, 1.0]);
        let flags = packet.change_flags[0];
        assert!(
            flags & CHANGED_TINT != 0,
            "expected CHANGED_TINT on removal, got {flags:#b}"
        );
    }

    #[test]
    fn incremental_extract_no_tint_flag_when_unchanged() {
        use crate::frame_packet::CHANGED_TINT;
        use galeon_engine::render::Tint;

        let mut world = World::new();
        let entity = world.spawn((
            Transform::from_position(1.0, 0.0, 0.0),
            Tint([0.5, 0.5, 0.5]),
        ));
        let since = world.change_tick();
        world.advance_tick();

        world.get_mut::<Transform>(entity).unwrap().position[0] = 99.0;

        let packet = extract_frame_incremental(&world, since);
        assert_eq!(packet.entity_count(), 1);
        let flags = packet.change_flags[0];
        assert!(flags & CHANGED_TRANSFORM != 0);
        assert!(flags & CHANGED_TINT == 0);
    }

    #[test]
    fn drain_clears_pending_for_next_frame() {
        let mut world = World::new();
        world.add_event::<TestImpact>();

        let mut registry = galeon_engine::RenderEventRegistry::new();
        registry.register::<TestImpact>();
        world.insert_resource(registry);
        world.spawn((Transform::identity(),));

        world
            .resource_mut::<galeon_engine::Events<TestImpact>>()
            .send(TestImpact {
                entity_index: 1,
                pos: [0.0; 3],
                force: 1.0,
            });
        world.flush_render_events();

        // Frame 1: drain returns the event.
        let packet1 = extract_frame(&world);
        assert_eq!(packet1.event_count(), 1);

        // Frame 2: no new events, drain returns empty.
        let packet2 = extract_frame(&world);
        assert_eq!(packet2.event_count(), 0);
    }
}
