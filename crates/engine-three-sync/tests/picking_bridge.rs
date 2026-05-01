// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

//! Verifies the WASM-bridge picking surface drives the same `Selection`
//! resource as the native API, lazy-installing the resource on first use and
//! exposing the selected entities as a flat `[idx, gen, …]` packing.

use galeon_engine::{
    MaterialHandle, MeshHandle, ObjectType, PickModifiers, Selection, Transform, Visibility,
};
use galeon_engine_three_sync::WasmEngine;

fn spawn_n_cubes(engine: &mut WasmEngine, n: u32) -> Vec<(u32, u32)> {
    let world = engine.engine_mut().world_mut();
    (0..n)
        .map(|i| {
            let entity = world.spawn((
                Transform {
                    position: [i as f32, 0.0, 0.0],
                    rotation: [0.0, 0.0, 0.0, 1.0],
                    scale: [1.0, 1.0, 1.0],
                },
                Visibility { visible: true },
                MeshHandle { id: 1 },
                MaterialHandle { id: 1 },
                ObjectType::Mesh,
            ));
            (entity.index(), entity.generation())
        })
        .collect()
}

#[test]
fn apply_pick_lazy_installs_selection_resource() {
    let mut engine = WasmEngine::new();
    let cubes = spawn_n_cubes(&mut engine, 5);
    assert!(
        engine
            .engine()
            .world()
            .try_resource::<Selection>()
            .is_none(),
        "selection resource should not exist before first applyPick",
    );

    let (idx, generation) = cubes[2];
    engine.apply_pick(true, idx, generation, false, 0.0, 0.0, 0.0, 0);
    assert_eq!(engine.selection_count(), 1);

    let flat = engine.selection_entities();
    assert_eq!(flat, vec![idx, generation]);
}

#[test]
fn apply_pick_rect_with_shift_adds_to_selection() {
    let mut engine = WasmEngine::new();
    let cubes = spawn_n_cubes(&mut engine, 5);

    // Click cube 0 first.
    engine.apply_pick(true, cubes[0].0, cubes[0].1, false, 0.0, 0.0, 0.0, 0);
    // Then shift+marquee cubes 1..4.
    let mut flat = Vec::new();
    for &(idx, generation) in &cubes[1..4] {
        flat.push(idx);
        flat.push(generation);
    }
    engine.apply_pick_rect(&flat, PickModifiers::SHIFT);

    assert_eq!(engine.selection_count(), 4);
}

#[test]
fn apply_pick_with_no_modifier_replaces() {
    let mut engine = WasmEngine::new();
    let cubes = spawn_n_cubes(&mut engine, 3);
    engine.apply_pick(true, cubes[0].0, cubes[0].1, false, 0.0, 0.0, 0.0, 0);
    engine.apply_pick(true, cubes[1].0, cubes[1].1, false, 0.0, 0.0, 0.0, 0);
    assert_eq!(engine.selection_count(), 1);
    assert_eq!(engine.selection_entities(), vec![cubes[1].0, cubes[1].1]);
}

#[test]
fn apply_pick_records_world_point_in_selection() {
    let mut engine = WasmEngine::new();
    let cubes = spawn_n_cubes(&mut engine, 1);
    let (idx, generation) = cubes[0];
    engine.apply_pick(true, idx, generation, true, 1.5, -2.5, 3.0, 0);

    let sel = engine
        .engine()
        .world()
        .try_resource::<Selection>()
        .expect("selection installed by applyPick");
    let pt = sel.last_pick.expect("last_pick recorded");
    assert_eq!(pt.x, 1.5);
    assert_eq!(pt.y, -2.5);
    assert_eq!(pt.z, 3.0);
}

#[test]
fn selection_entities_returns_empty_when_resource_missing() {
    let engine = WasmEngine::new();
    assert_eq!(engine.selection_count(), 0);
    assert!(engine.selection_entities().is_empty());
}

#[test]
fn apply_pick_rect_ignores_trailing_odd_element() {
    let mut engine = WasmEngine::new();
    let cubes = spawn_n_cubes(&mut engine, 3);
    // Two complete pairs plus a stray u32 — should select 2 entities, not panic.
    let flat = vec![
        cubes[0].0, cubes[0].1, cubes[1].0, cubes[1].1, // dangling odd element
        99,
    ];
    engine.apply_pick_rect(&flat, 0);
    assert_eq!(engine.selection_count(), 2);
}
