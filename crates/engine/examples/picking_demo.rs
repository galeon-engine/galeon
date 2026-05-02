// SPDX-License-Identifier: AGPL-3.0-only OR Commercial
//
//! Picking demo — 50 cubes, simulated pick events, observable [`Selection`] state.
//!
//! Demonstrates the data flow that the `@galeon/picking` TS helper drives in
//! the browser:
//!
//! 1. Spawn a 50-cube scene with renderable components.
//! 2. Insert a [`Selection`] resource.
//! 3. Simulate a single-click pick — clicking entity 7 with no modifier.
//! 4. Simulate a marquee pick — selecting cubes 10..20 with shift held.
//! 5. Observe the resource state via [`Engine::world`] and the WASM-bridge
//!    debug snapshot.
//!
//! This is the native verification path for #214 T4. The browser path runs the
//! same `Selection::apply_pick` / `apply_pick_rect` calls through the WASM
//! bridge, but the resource state and modifier semantics are identical.

use galeon_engine::{
    Engine, MaterialHandle, MeshHandle, ObjectType, PickModifiers, PickPoint, Selection, Transform,
    Visibility,
};

fn main() {
    let mut engine = Engine::new();
    let mut entities = Vec::with_capacity(50);

    // Spawn a 5×10 grid of cubes — covers a recognisable patch of world space.
    for i in 0..50 {
        let row = (i / 10) as f32;
        let col = (i % 10) as f32;
        let entity = engine.world_mut().spawn((
            Transform {
                position: [col - 4.5, row - 2.0, 0.0],
                rotation: [0.0, 0.0, 0.0, 1.0],
                scale: [1.0, 1.0, 1.0],
            },
            Visibility { visible: true },
            MeshHandle { id: 1 },
            MaterialHandle { id: 1 },
            ObjectType::Mesh,
        ));
        entities.push(entity);
    }
    println!("spawned {} cubes", entities.len());

    engine.world_mut().insert_resource(Selection::new());

    // ---- 1. Single-click pick on entity 7, no modifier ---------------------
    {
        let target = entities[7];
        let selection = engine.world_mut().resource_mut::<Selection>();
        selection.apply_pick(
            Some(target),
            Some(PickPoint {
                x: 2.5,
                y: -2.0,
                z: 0.0,
            }),
            PickModifiers::NONE,
        );
        println!(
            "after click on cube[7]: selected={}, last_pick={:?}",
            selection.len(),
            selection.last_pick,
        );
        assert_eq!(selection.len(), 1);
        assert!(selection.contains(target));
    }

    // ---- 2. Shift+marquee adds cubes 10..20 to the selection ---------------
    {
        let marquee: Vec<_> = entities[10..20].to_vec();
        let selection = engine.world_mut().resource_mut::<Selection>();
        selection.apply_pick_rect(marquee.iter().copied(), PickModifiers(PickModifiers::SHIFT));
        println!(
            "after shift+marquee on cubes[10..20]: selected={}",
            selection.len(),
        );
        assert_eq!(selection.len(), 11); // 1 from the click + 10 from the rect
    }

    // ---- 3. Ctrl+click on entity 7 removes it ------------------------------
    {
        let target = entities[7];
        let selection = engine.world_mut().resource_mut::<Selection>();
        selection.apply_pick(Some(target), None, PickModifiers(PickModifiers::CTRL));
        println!("after ctrl+click on cube[7]: selected={}", selection.len(),);
        assert_eq!(selection.len(), 10);
        assert!(!selection.contains(target));
    }

    // ---- 4. Click on empty space clears the selection ----------------------
    {
        let selection = engine.world_mut().resource_mut::<Selection>();
        selection.apply_pick(None, None, PickModifiers::NONE);
        println!("after click on empty space: selected={}", selection.len());
        assert!(selection.is_empty());
    }

    println!("picking demo OK");
}
