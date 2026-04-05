// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

//! Debug snapshot for tooling — human-readable JSON representation of
//! render-facing world state. NOT used in the render hot path.

use std::collections::HashMap;

use galeon_engine::RenderChannelRegistry;
use galeon_engine::World;
use galeon_engine::render::{MaterialHandle, MeshHandle, ObjectType, Transform, Visibility};
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
    pub object_type: Option<String>,
    pub custom_channels: HashMap<String, Vec<f32>>,
}

/// Human-readable transform with named fields.
#[derive(Debug, Serialize)]
pub struct TransformSnapshot {
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
}

/// Extract a debug snapshot from the world.
///
/// Single-pass query using optional components: iterates all entities with a
/// `Transform` component (same filter as the render hot path) and serializes
/// their render-facing data into a human-readable structure.
pub fn extract_debug_snapshot(world: &World) -> DebugSnapshot {
    let query = world.query::<(
        &Transform,
        Option<&Visibility>,
        Option<&MeshHandle>,
        Option<&MaterialHandle>,
    )>();

    let registry = world.try_resource::<RenderChannelRegistry>();
    let mut entities = Vec::with_capacity(query.len());

    for (entity, (transform, vis, mesh, mat)) in query {
        let obj_type = world.get::<ObjectType>(entity);
        let mut custom_channels = HashMap::new();
        if let Some(reg) = registry {
            for channel in &reg.channels {
                let mut buf = vec![0.0f32; channel.stride];
                (channel.extract_fn)(world, entity, &mut buf);
                custom_channels.insert(channel.name.clone(), buf);
            }
        }
        entities.push(EntitySnapshot {
            id: entity.index(),
            generation: entity.generation(),
            transform: Some(TransformSnapshot {
                position: transform.position,
                rotation: transform.rotation,
                scale: transform.scale,
            }),
            visible: vis.map(|v| v.visible),
            mesh_handle: mesh.map(|m| m.id),
            material_handle: mat.map(|m| m.id),
            object_type: obj_type.map(|t| format!("{:?}", t)),
            custom_channels,
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
    use galeon_engine::ObjectType;
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

    #[derive(Debug)]
    struct HeatLevel {
        value: f32,
    }

    impl galeon_engine::Component for HeatLevel {}

    impl galeon_engine::ExtractToFloats for HeatLevel {
        const STRIDE: usize = 1;
        fn extract(&self, buf: &mut [f32]) {
            buf[0] = self.value;
        }
    }

    #[test]
    fn snapshot_includes_custom_channels() {
        let mut world = World::new();
        let mut reg = galeon_engine::RenderChannelRegistry::new();
        reg.register::<HeatLevel>("heat");
        world.insert_resource(reg);

        world.spawn((
            Transform::from_position(1.0, 0.0, 0.0),
            HeatLevel { value: 0.9 },
        ));

        let snap = extract_debug_snapshot(&world);
        let e = &snap.entities[0];
        assert_eq!(e.custom_channels.len(), 1);
        let heat = &e.custom_channels["heat"];
        assert!((heat[0] - 0.9).abs() < f32::EPSILON);
    }

    #[test]
    fn snapshot_custom_channels_empty_when_no_registry() {
        let mut world = World::new();
        world.spawn((Transform::identity(),));
        let snap = extract_debug_snapshot(&world);
        assert!(snap.entities[0].custom_channels.is_empty());
    }

    #[test]
    fn snapshot_includes_object_type() {
        let mut world = World::new();
        world.spawn((
            Transform::from_position(1.0, 0.0, 0.0),
            ObjectType::PointLight,
        ));

        let snap = extract_debug_snapshot(&world);
        let e = &snap.entities[0];
        assert_eq!(e.object_type, Some("PointLight".to_string()));
    }

    #[test]
    fn snapshot_object_type_none_when_absent() {
        let mut world = World::new();
        world.spawn((Transform::identity(),));

        let snap = extract_debug_snapshot(&world);
        assert_eq!(snap.entities[0].object_type, None);
    }
}
