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
fn query_iter_yields_matching_entities() {
    let mut world = World::new();
    world.spawn((Pos { x: 1.0, y: 0.0 },));
    world.spawn((Pos { x: 2.0, y: 0.0 }, Vel { dx: 0.0, dy: 0.0 }));
    world.spawn((Vel { dx: 3.0, dy: 0.0 },)); // no Pos

    // query() returns a lazy iterator — no Vec allocation.
    let positions: Vec<f32> = world.query::<Pos>().map(|(_, p)| p.x).collect();
    assert_eq!(positions.len(), 2);
    assert!(positions.contains(&1.0));
    assert!(positions.contains(&2.0));
}

#[test]
fn query_iter_empty_world() {
    let world = World::new();
    let results: Vec<_> = world.query::<Pos>().collect();
    assert!(results.is_empty());
}

#[test]
fn query_iter_entity_is_copy_not_ref() {
    let mut world = World::new();
    let e = world.spawn((Pos { x: 5.0, y: 0.0 },));

    // Entity from iterator is owned (Copy), not a reference.
    let (entity, pos) = world.query::<Pos>().next().unwrap();
    assert_eq!(entity, e);
    assert_eq!(pos.x, 5.0);
}

#[test]
fn query_iter_mut_allows_modification() {
    let mut world = World::new();
    world.spawn((Pos { x: 0.0, y: 0.0 },));
    world.spawn((Pos { x: 10.0, y: 10.0 },));

    for (_, pos) in world.query_mut::<Pos>() {
        pos.x += 1.0;
    }

    let xs: Vec<f32> = world.query::<Pos>().map(|(_, p)| p.x).collect();
    assert!(xs.contains(&1.0));
    assert!(xs.contains(&11.0));
}

#[test]
fn query_iter_mut_empty_world() {
    let mut world = World::new();
    let results: Vec<_> = world.query_mut::<Pos>().collect();
    assert!(results.is_empty());
}

#[test]
fn query2_iter_yields_entities_with_both() {
    let mut world = World::new();
    world.spawn((Pos { x: 1.0, y: 0.0 },));
    world.spawn((Pos { x: 2.0, y: 0.0 }, Vel { dx: 5.0, dy: 0.0 }));
    world.spawn((Vel { dx: 3.0, dy: 0.0 },));

    let results: Vec<_> = world.query2::<Pos, Vel>().collect();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].1.x, 2.0);
    assert_eq!(results[0].2.dx, 5.0);
}

#[test]
fn query2_iter_empty_when_no_overlap() {
    let mut world = World::new();
    world.spawn((Pos { x: 1.0, y: 0.0 },));
    world.spawn((Vel { dx: 2.0, dy: 0.0 },));

    let results: Vec<_> = world.query2::<Pos, Vel>().collect();
    assert!(results.is_empty());
}
