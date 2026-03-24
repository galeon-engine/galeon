// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

use galeon_engine::{Component, World};

#[derive(Component, Debug, Clone, PartialEq)]
struct Pos {
    x: f32,
    y: f32,
}

#[derive(Component, Debug, Clone, PartialEq)]
struct Vel {
    dx: f32,
    dy: f32,
}

#[test]
fn query_yields_matching_entities() {
    let mut world = World::new();
    world.spawn((Pos { x: 1.0, y: 0.0 },));
    world.spawn((Pos { x: 2.0, y: 0.0 }, Vel { dx: 0.0, dy: 0.0 }));
    world.spawn((Vel { dx: 3.0, dy: 0.0 },)); // no Pos

    let positions: Vec<f32> = world.query::<Pos>().into_iter().map(|(_, p)| p.x).collect();
    assert_eq!(positions.len(), 2);
    assert!(positions.contains(&1.0));
    assert!(positions.contains(&2.0));
}

#[test]
fn query_empty_world() {
    let world = World::new();
    let results = world.query::<Pos>();
    assert!(results.is_empty());
}

#[test]
fn query_skips_despawned_entities() {
    let mut world = World::new();
    let e1 = world.spawn((Pos { x: 1.0, y: 0.0 },));
    let _e2 = world.spawn((Pos { x: 2.0, y: 0.0 },));
    world.despawn(e1);

    let results = world.query::<Pos>();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].1.x, 2.0);
}

#[test]
fn query_entity_is_copy_not_ref() {
    let mut world = World::new();
    let e = world.spawn((Pos { x: 5.0, y: 0.0 },));

    let results = world.query::<Pos>();
    let (entity, pos) = &results[0];
    assert_eq!(*entity, e);
    assert_eq!(pos.x, 5.0);
}

#[test]
fn query_mut_allows_modification() {
    let mut world = World::new();
    world.spawn((Pos { x: 0.0, y: 0.0 },));
    world.spawn((Pos { x: 10.0, y: 10.0 },));

    for (_, pos) in world.query_mut::<Pos>() {
        pos.x += 1.0;
    }

    let xs: Vec<f32> = world.query::<Pos>().into_iter().map(|(_, p)| p.x).collect();
    assert!(xs.contains(&1.0));
    assert!(xs.contains(&11.0));
}

#[test]
fn query_mut_empty_world() {
    let mut world = World::new();
    let results = world.query_mut::<Pos>();
    assert!(results.is_empty());
}

#[test]
fn query2_yields_entities_with_both() {
    let mut world = World::new();
    world.spawn((Pos { x: 1.0, y: 0.0 },));
    world.spawn((Pos { x: 2.0, y: 0.0 }, Vel { dx: 5.0, dy: 0.0 }));
    world.spawn((Vel { dx: 3.0, dy: 0.0 },));

    let results = world.query2::<Pos, Vel>();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].1.x, 2.0);
    assert_eq!(results[0].2.dx, 5.0);
}

#[test]
fn query2_empty_when_no_overlap() {
    let mut world = World::new();
    world.spawn((Pos { x: 1.0, y: 0.0 },));
    world.spawn((Vel { dx: 2.0, dy: 0.0 },));

    let results = world.query2::<Pos, Vel>();
    assert!(results.is_empty());
}

#[test]
fn query2_mut_mutates_both() {
    let mut world = World::new();
    let e = world.spawn((Pos { x: 1.0, y: 1.0 }, Vel { dx: 10.0, dy: 10.0 }));

    for (_, pos, vel) in world.query2_mut::<Pos, Vel>() {
        pos.x += 100.0;
        vel.dy += 200.0;
    }

    assert_eq!(world.get::<Pos>(e).unwrap().x, 101.0);
    assert_eq!(world.get::<Vel>(e).unwrap().dy, 210.0);
}

#[test]
fn query2_mut_skips_missing() {
    let mut world = World::new();
    world.spawn((Pos { x: 5.0, y: 0.0 },));
    let e = world.spawn((Pos { x: 7.0, y: 0.0 }, Vel { dx: 9.0, dy: 0.0 }));
    world.spawn((Vel { dx: 11.0, dy: 0.0 },));

    let results = world.query2_mut::<Pos, Vel>();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].0, e);
}

#[test]
#[should_panic(expected = "cannot borrow the same column mutably twice")]
fn query2_mut_same_type_panics() {
    let mut world = World::new();
    world.spawn((Pos { x: 0.0, y: 0.0 },));
    let _ = world.query2_mut::<Pos, Pos>();
}
