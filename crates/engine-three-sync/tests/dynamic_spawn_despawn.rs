// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

use galeon_engine::{Engine, MaterialHandle, MeshHandle, Plugin, Transform, Visibility};
use galeon_engine_three_sync::WasmEngine;

const PLUGIN_MESH: u32 = 7;
const PLUGIN_MATERIAL: u32 = 11;
const DYNAMIC_MESH: u32 = 42;
const DYNAMIC_MATERIAL: u32 = 99;

/// Identity transform packed as 10 f32: pos3 + rot4 + scale3.
const IDENTITY: [f32; 10] = [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0];

/// A plugin that seeds one entity at build time.
struct SeedPlugin;

impl Plugin for SeedPlugin {
    fn build(&self, engine: &mut Engine) {
        engine.world_mut().spawn((
            Transform::identity(),
            Visibility { visible: true },
            MeshHandle { id: PLUGIN_MESH },
            MaterialHandle {
                id: PLUGIN_MATERIAL,
            },
        ));
    }
}

fn wasm_engine_with_seed() -> WasmEngine {
    let mut w = WasmEngine::new();
    w.engine_mut().add_plugin(SeedPlugin);
    w
}

// -------------------------------------------------------------------------
// spawn_entity
// -------------------------------------------------------------------------

#[test]
fn spawn_entity_returns_valid_id() {
    let mut w = WasmEngine::new();
    let id = w.spawn_entity(DYNAMIC_MESH, DYNAMIC_MATERIAL, &IDENTITY, 0);
    assert_eq!(id.len(), 2);
}

#[test]
fn spawn_entity_rejects_short_transform() {
    let mut w = WasmEngine::new();
    // 9 elements — too short, should return empty (not panic)
    let id = w.spawn_entity(DYNAMIC_MESH, DYNAMIC_MATERIAL, &[0.0; 9], 0);
    assert!(id.is_empty());
    assert_eq!(w.extract_frame().entity_count(), 0);
}

#[test]
fn spawn_entity_rejects_empty_transform() {
    let mut w = WasmEngine::new();
    let id = w.spawn_entity(DYNAMIC_MESH, DYNAMIC_MATERIAL, &[], 0);
    assert!(id.is_empty());
}

#[test]
fn spawn_entity_appears_in_extract_frame() {
    let mut w = WasmEngine::new();
    let xform = [1.0, 2.0, 3.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0];
    w.spawn_entity(DYNAMIC_MESH, DYNAMIC_MATERIAL, &xform, 0);

    let frame = w.extract_frame();
    assert_eq!(frame.entity_count(), 1);
    assert_eq!(frame.mesh_handles(), vec![DYNAMIC_MESH]);
    assert_eq!(frame.material_handles(), vec![DYNAMIC_MATERIAL]);
    assert_eq!(frame.visibility(), vec![1]);

    // Check transform: pos = [1,2,3], rot = [0,0,0,1], scale = [1,1,1]
    let t = frame.transforms();
    assert_eq!(t.len(), 10);
    assert_eq!(&t[0..3], &[1.0, 2.0, 3.0]);
    assert_eq!(&t[3..7], &[0.0, 0.0, 0.0, 1.0]);
    assert_eq!(&t[7..10], &[1.0, 1.0, 1.0]);
}

#[test]
fn multiple_spawns_all_visible() {
    let mut w = WasmEngine::new();
    w.spawn_entity(1, 10, &IDENTITY, 0);
    w.spawn_entity(2, 20, &IDENTITY, 0);
    w.spawn_entity(3, 30, &IDENTITY, 0);

    let frame = w.extract_frame();
    assert_eq!(frame.entity_count(), 3);
}

#[test]
fn js_entity_count_tracks_spawns() {
    let mut w = WasmEngine::new();
    assert_eq!(w.js_entity_count(), 0);

    w.spawn_entity(1, 10, &IDENTITY, 0);
    assert_eq!(w.js_entity_count(), 1);

    w.spawn_entity(2, 20, &IDENTITY, 0);
    assert_eq!(w.js_entity_count(), 2);
}

// -------------------------------------------------------------------------
// despawn_entity
// -------------------------------------------------------------------------

#[test]
fn despawn_entity_removes_from_frame() {
    let mut w = WasmEngine::new();
    let id = w.spawn_entity(DYNAMIC_MESH, DYNAMIC_MATERIAL, &IDENTITY, 0);

    assert!(w.despawn_entity(id[0], id[1]));

    let frame = w.extract_frame();
    assert_eq!(frame.entity_count(), 0);
    assert_eq!(w.js_entity_count(), 0);
}

#[test]
fn despawn_stale_handle_returns_false() {
    let mut w = WasmEngine::new();
    let id = w.spawn_entity(DYNAMIC_MESH, DYNAMIC_MATERIAL, &IDENTITY, 0);

    assert!(w.despawn_entity(id[0], id[1]));
    // Stale handle — generation mismatch
    assert!(!w.despawn_entity(id[0], id[1]));
}

#[test]
fn despawn_nonexistent_entity_returns_false() {
    let mut w = WasmEngine::new();
    assert!(!w.despawn_entity(999, 0));
}

// -------------------------------------------------------------------------
// Lifecycle guards — JS cannot despawn plugin entities
// -------------------------------------------------------------------------

#[test]
fn despawn_rejects_plugin_spawned_entity() {
    let mut w = wasm_engine_with_seed();

    // The plugin entity is at index 0, generation 0
    assert!(!w.despawn_entity(0, 0));

    // Plugin entity still present
    let frame = w.extract_frame();
    assert_eq!(frame.entity_count(), 1);
    assert_eq!(frame.mesh_handles(), vec![PLUGIN_MESH]);
}

#[test]
fn js_entity_count_excludes_plugin_entities() {
    let mut w = wasm_engine_with_seed();
    assert_eq!(w.js_entity_count(), 0);

    w.spawn_entity(DYNAMIC_MESH, DYNAMIC_MATERIAL, &IDENTITY, 0);
    assert_eq!(w.js_entity_count(), 1);

    // Total entities = 2 (plugin + JS), but js_entity_count = 1
    let frame = w.extract_frame();
    assert_eq!(frame.entity_count(), 2);
}

// -------------------------------------------------------------------------
// Bulk cleanup
// -------------------------------------------------------------------------

#[test]
fn despawn_all_js_entities_cleans_up() {
    let mut w = wasm_engine_with_seed();
    w.spawn_entity(1, 10, &IDENTITY, 0);
    w.spawn_entity(2, 20, &IDENTITY, 0);
    w.spawn_entity(3, 30, &IDENTITY, 0);

    assert_eq!(w.js_entity_count(), 3);
    let removed = w.despawn_all_js_entities();
    assert_eq!(removed, 3);
    assert_eq!(w.js_entity_count(), 0);

    // Plugin entity survives
    let frame = w.extract_frame();
    assert_eq!(frame.entity_count(), 1);
    assert_eq!(frame.mesh_handles(), vec![PLUGIN_MESH]);
}

#[test]
fn despawn_all_js_entities_returns_zero_when_none() {
    let mut w = wasm_engine_with_seed();
    assert_eq!(w.despawn_all_js_entities(), 0);
}

// -------------------------------------------------------------------------
// Plugin-spawned entities are unaffected
// -------------------------------------------------------------------------

#[test]
fn plugin_entities_survive_dynamic_spawn_and_despawn() {
    let mut w = wasm_engine_with_seed();

    let id = w.spawn_entity(
        DYNAMIC_MESH,
        DYNAMIC_MATERIAL,
        &[5.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        0,
    );

    let frame = w.extract_frame();
    assert_eq!(frame.entity_count(), 2);

    assert!(w.despawn_entity(id[0], id[1]));

    let frame = w.extract_frame();
    assert_eq!(frame.entity_count(), 1);
    assert_eq!(frame.mesh_handles(), vec![PLUGIN_MESH]);
    assert_eq!(frame.material_handles(), vec![PLUGIN_MATERIAL]);
}

#[test]
fn dynamic_entity_does_not_corrupt_plugin_entity_data() {
    let mut w = wasm_engine_with_seed();

    for i in 0..5 {
        let id = w.spawn_entity(100 + i, 200 + i, &IDENTITY, 0);
        w.despawn_entity(id[0], id[1]);
    }

    let frame = w.extract_frame();
    assert_eq!(frame.entity_count(), 1);
    assert_eq!(frame.mesh_handles(), vec![PLUGIN_MESH]);
    assert_eq!(frame.material_handles(), vec![PLUGIN_MATERIAL]);
}
