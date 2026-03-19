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
            1, // extract_frame always marks everything as changed
        );
    }

    packet
}

/// Extract a frame with change flags relative to `since_cursor`.
///
/// All entities with a `Transform` component are included in the packet
/// (the renderer needs the full scene graph to manage entity lifetimes), but
/// `change_flags[i]` is `1` only when at least one render-facing component
/// (`Transform`, `Visibility`, `MeshHandle`, or `MaterialHandle`) changed
/// after `since_cursor`, and `0` otherwise.
///
/// Capture the cursor with `World::current_change_cursor()` after an
/// extraction. Using `since_cursor = 0` is equivalent to `extract_frame` —
/// every entity gets `change_flag = 1`.
pub fn extract_frame_incremental(world: &World, since_cursor: u64) -> FramePacket {
    // First pass: collect entity + transform data into owned values.
    // Releasing the `query::<Transform>` borrow lets us call `get()` and
    // `component_changed_tick()` per entity in the second pass.
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

        // An entity is "changed" if ANY of its render-facing components has a
        // change cursor strictly greater than `since_cursor`.
        let transform_changed = world
            .component_changed_cursor::<Transform>(*entity)
            .map(|cursor| cursor > since_cursor)
            .unwrap_or(false);
        let visibility_changed = world
            .component_changed_cursor::<Visibility>(*entity)
            .map(|cursor| cursor > since_cursor)
            .unwrap_or(false);
        let mesh_changed = world
            .component_changed_cursor::<MeshHandle>(*entity)
            .map(|cursor| cursor > since_cursor)
            .unwrap_or(false);
        let material_changed = world
            .component_changed_cursor::<MaterialHandle>(*entity)
            .map(|cursor| cursor > since_cursor)
            .unwrap_or(false);

        let change_flag =
            (transform_changed || visibility_changed || mesh_changed || material_changed) as u8;

        packet.push(
            entity.index(),
            entity.generation(),
            position,
            rotation,
            scale,
            visible,
            mesh_id,
            material_id,
            change_flag,
        );
    }

    packet
}

#[cfg(test)]
mod tests {
    use super::*;
    use galeon_engine::Engine;
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
        // extract_frame always marks everything changed
        assert_eq!(packet.change_flags[0], 1);
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
    // Incremental extraction tests
    // -------------------------------------------------------------------------

    /// First extraction with since: 0 — every entity should have change_flag = 1,
    /// because all components have changed_tick >= 1 > 0.
    #[test]
    fn incremental_first_extraction_all_changed() {
        let mut world = World::new();
        world.spawn((Transform::from_position(1.0, 0.0, 0.0),));
        world.spawn((Transform::from_position(2.0, 0.0, 0.0),));

        let packet = extract_frame_incremental(&world, 0);
        assert_eq!(packet.entity_count(), 2);
        assert_eq!(packet.change_flags[0], 1);
        assert_eq!(packet.change_flags[1], 1);
    }

    /// After extraction at the current tick, a second extraction with no
    /// mutations between them should yield all change_flags = 0.
    #[test]
    fn incremental_no_mutations_all_unchanged() {
        let mut world = World::new();
        world.spawn((Transform::from_position(1.0, 0.0, 0.0),));
        world.spawn((Transform::from_position(2.0, 0.0, 0.0),));

        // Record change cursor after first extraction.
        let since = world.current_change_cursor();

        // Second extraction — nothing mutated, so all flags should be 0.
        let packet = extract_frame_incremental(&world, since);
        assert_eq!(packet.entity_count(), 2);
        assert_eq!(packet.change_flags[0], 0);
        assert_eq!(packet.change_flags[1], 0);
    }

    /// Mutation between extractions marks only the mutated entity as changed.
    #[test]
    fn incremental_selective_mutation() {
        let mut engine = Engine::new();
        let e1 = engine
            .world_mut()
            .spawn((Transform::from_position(1.0, 0.0, 0.0),));
        let _e2 = engine
            .world_mut()
            .spawn((Transform::from_position(2.0, 0.0, 0.0),));

        // First extraction — capture the current change cursor.
        let since = engine.world().current_change_cursor();

        // Advance tick and mutate only e1's transform.
        engine.run_once(); // tick → 2
        engine
            .world_mut()
            .get_mut::<Transform>(e1)
            .unwrap()
            .position = [99.0, 0.0, 0.0];

        // Extract incrementally: only e1 should be flagged.
        let packet = extract_frame_incremental(engine.world(), since);
        assert_eq!(packet.entity_count(), 2);

        // Find e1 and e2 by their entity IDs in the packet.
        let e1_pos = packet
            .entity_ids
            .iter()
            .position(|&id| id == e1.index())
            .expect("e1 must be in packet");
        let e1_flag = packet.change_flags[e1_pos];
        assert_eq!(e1_flag, 1, "e1 should be flagged as changed");

        // The other entity (e2) should be unchanged.
        let e2_flag = packet
            .change_flags
            .iter()
            .enumerate()
            .find(|(i, _)| *i != e1_pos)
            .map(|(_, &f)| f)
            .expect("e2 must be in packet");
        assert_eq!(e2_flag, 0, "e2 should not be flagged as changed");
    }

    /// Changing a non-Transform render component (e.g. Visibility) also flags
    /// the entity as changed in the incremental packet.
    #[test]
    fn incremental_visibility_change_flags_entity() {
        let mut engine = Engine::new();
        let e = engine
            .world_mut()
            .spawn((Transform::identity(), Visibility { visible: true }));

        let since = engine.world().current_change_cursor();

        // Mutate Visibility — not Transform.
        engine.world_mut().get_mut::<Visibility>(e).unwrap().visible = false;

        let packet = extract_frame_incremental(engine.world(), since);
        assert_eq!(packet.entity_count(), 1);
        assert_eq!(packet.change_flags[0], 1);
        assert_eq!(packet.visibility[0], 0); // confirm the value is also updated
    }

    /// Mutations within the same schedule tick must still be visible to
    /// incremental extraction as long as the caller snapshots the change cursor
    /// before the mutation.
    #[test]
    fn incremental_same_tick_mutation_uses_change_cursor() {
        let mut world = World::new();
        let e = world.spawn((Transform::identity(),));

        let since = world.current_change_cursor();
        world.get_mut::<Transform>(e).unwrap().position = [42.0, 0.0, 0.0];

        let packet = extract_frame_incremental(&world, since);
        assert_eq!(packet.entity_count(), 1);
        assert_eq!(packet.change_flags[0], 1);
        assert_eq!(packet.transforms[0], 42.0);
    }

    /// Incremental extraction on an empty world produces an empty packet.
    #[test]
    fn incremental_empty_world() {
        let world = World::new();
        let packet = extract_frame_incremental(&world, 0);
        assert_eq!(packet.entity_count(), 0);
        assert!(packet.change_flags.is_empty());
    }
}
