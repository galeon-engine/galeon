// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

//! Debug snapshot for tooling — human-readable JSON representation of
//! render-facing world state. NOT used in the render hot path.

use galeon_engine::World;
use galeon_engine::render::{MaterialHandle, MeshHandle, Transform, Visibility};
use serde::Serialize;

/// Debug snapshot of all renderable entities.
///
/// Designed for inspector panels, profiler overlays, and shell tooling.
/// Uses named fields (not flat arrays) for readability.
#[derive(Debug, Serialize)]
pub struct DebugSnapshot {
    pub engine_version: String,
    pub entity_count: usize,
    pub entities: Vec<EntitySnapshot>,
}

/// Per-entity debug data with all render-facing components.
#[derive(Debug, Serialize)]
pub struct EntitySnapshot {
    pub id: u32,
    pub generation: u32,
    pub transform: Option<TransformSnapshot>,
    pub visible: Option<bool>,
    pub mesh_handle: Option<u32>,
    pub material_handle: Option<u32>,
}

/// Human-readable transform with named fields.
#[derive(Debug, Serialize)]
pub struct TransformSnapshot {
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
}

/// Owned copy of a single entity's transform, held between the query borrow
/// and the optional-component lookups.
struct RawTransform {
    entity: galeon_engine::Entity,
    position: [f32; 3],
    rotation: [f32; 4],
    scale: [f32; 3],
}

/// Extract a debug snapshot from the world.
///
/// Iterates all entities with a `Transform` component (same filter as the
/// render hot path) and serializes their render-facing data into a
/// human-readable structure.
pub fn extract_debug_snapshot(world: &World) -> DebugSnapshot {
    // Collect transform entities into owned data first. The `QueryIter`
    // borrows `world.archetypes`; `.collect()` consumes it and releases that
    // borrow so we can call `world.get()` per entity below.
    let renderables: Vec<RawTransform> = world
        .query::<&Transform>()
        .map(|(e, t)| RawTransform {
            entity: e,
            position: t.position,
            rotation: t.rotation,
            scale: t.scale,
        })
        .collect();

    let mut entities = Vec::with_capacity(renderables.len());

    for raw in &renderables {
        entities.push(EntitySnapshot {
            id: raw.entity.index(),
            generation: raw.entity.generation(),
            transform: Some(TransformSnapshot {
                position: raw.position,
                rotation: raw.rotation,
                scale: raw.scale,
            }),
            visible: world.get::<Visibility>(raw.entity).map(|v| v.visible),
            mesh_handle: world.get::<MeshHandle>(raw.entity).map(|m| m.id),
            material_handle: world.get::<MaterialHandle>(raw.entity).map(|m| m.id),
        });
    }

    DebugSnapshot {
        engine_version: galeon_engine::engine_version().to_string(),
        entity_count: entities.len(),
        entities,
    }
}

/// Serialize a debug snapshot to a JSON string.
pub fn snapshot_to_json(snapshot: &DebugSnapshot) -> String {
    serde_json::to_string_pretty(snapshot).expect("DebugSnapshot should always serialize")
}

#[cfg(test)]
mod tests {
    use super::*;
    use galeon_engine::render::{MaterialHandle, MeshHandle, Transform, Visibility};

    #[test]
    fn empty_world_snapshot() {
        let world = World::new();
        let snap = extract_debug_snapshot(&world);
        assert_eq!(snap.entity_count, 0);
        assert!(snap.entities.is_empty());
    }

    #[test]
    fn snapshot_with_all_components() {
        let mut world = World::new();
        world.spawn((
            Transform::from_position(1.0, 2.0, 3.0),
            Visibility { visible: true },
            MeshHandle { id: 10 },
            MaterialHandle { id: 20 },
        ));

        let snap = extract_debug_snapshot(&world);
        assert_eq!(snap.entity_count, 1);

        let e = &snap.entities[0];
        assert!(e.transform.is_some());
        assert_eq!(e.transform.as_ref().unwrap().position, [1.0, 2.0, 3.0]);
        assert_eq!(e.visible, Some(true));
        assert_eq!(e.mesh_handle, Some(10));
        assert_eq!(e.material_handle, Some(20));
    }

    #[test]
    fn snapshot_missing_optional_components() {
        let mut world = World::new();
        world.spawn((Transform::identity(),));

        let snap = extract_debug_snapshot(&world);
        let e = &snap.entities[0];
        assert!(e.transform.is_some());
        assert_eq!(e.visible, None);
        assert_eq!(e.mesh_handle, None);
        assert_eq!(e.material_handle, None);
    }

    #[test]
    fn snapshot_serializes_to_json() {
        let mut world = World::new();
        world.spawn((
            Transform::from_position(5.0, 0.0, 0.0),
            MeshHandle { id: 1 },
        ));

        let snap = extract_debug_snapshot(&world);
        let json = snapshot_to_json(&snap);
        assert!(json.contains("\"engine_version\""));
        assert!(json.contains("\"entity_count\": 1"));
        assert!(json.contains("\"position\""));
        assert!(json.contains("5.0"));
    }
}
