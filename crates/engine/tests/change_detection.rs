// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

use galeon_engine::{Component, Engine, World};

#[derive(Component, Debug, Clone, Copy, PartialEq)]
struct Pos {
    x: f32,
}

#[derive(Component, Debug, Clone, Copy, PartialEq)]
struct Tag(u32);

#[derive(Component, Debug)]
struct Unregistered;

#[test]
fn tick_starts_at_one() {
    let world = World::new();
    assert_eq!(world.current_tick(), 1);
}

#[test]
fn tick_advances_on_schedule_run() {
    let mut engine = Engine::new();
    assert_eq!(engine.world().current_tick(), 1);

    engine.run_once();
    assert_eq!(engine.world().current_tick(), 2);

    engine.run_once();
    assert_eq!(engine.world().current_tick(), 3);
}

#[test]
fn component_insert_records_tick() {
    let mut world = World::new();
    // World tick is 1 at creation
    let e = world.spawn((Pos { x: 0.0 },));

    // Component should have been inserted at tick 1
    assert_eq!(world.component_added_tick::<Pos>(e), Some(1));
    assert_eq!(world.component_changed_tick::<Pos>(e), Some(1));
}

#[test]
fn get_mut_marks_changed_tick() {
    let mut engine = Engine::new();
    let e = engine.world_mut().spawn((Pos { x: 0.0 },));

    // Inserted at tick 1
    assert_eq!(engine.world().component_changed_tick::<Pos>(e), Some(1));

    // Run schedule to advance tick to 2
    engine.run_once();

    // Mutate via get_mut
    engine.world_mut().get_mut::<Pos>(e).unwrap().x = 5.0;

    // Changed tick should now be 2
    assert_eq!(engine.world().component_changed_tick::<Pos>(e), Some(2));
}

#[test]
fn query_mut_marks_all_changed() {
    let mut engine = Engine::new();
    let e1 = engine.world_mut().spawn((Pos { x: 1.0 },));
    let e2 = engine.world_mut().spawn((Pos { x: 2.0 },));

    // Both inserted at tick 1
    assert_eq!(engine.world().component_changed_tick::<Pos>(e1), Some(1));

    engine.run_once(); // tick → 2

    // query_mut touches all Pos components
    let _ = engine.world_mut().query_mut::<Pos>();

    // Both should be marked at tick 2
    assert_eq!(engine.world().component_changed_tick::<Pos>(e1), Some(2));
    assert_eq!(engine.world().component_changed_tick::<Pos>(e2), Some(2));
}

#[test]
fn query_changed_filters_by_tick() {
    let mut engine = Engine::new();
    let e1 = engine.world_mut().spawn((Pos { x: 1.0 },));
    let _e2 = engine.world_mut().spawn((Pos { x: 2.0 },));

    // Both at tick 1. query_changed(since: 0) should return both.
    let changed: Vec<_> = engine.world().query_changed::<Pos>(0);
    assert_eq!(changed.len(), 2);

    engine.run_once(); // tick → 2

    // Mutate only e1
    engine.world_mut().get_mut::<Pos>(e1).unwrap().x = 99.0;

    // query_changed(since: 1) should return only e1
    let changed: Vec<_> = engine.world().query_changed::<Pos>(1);
    assert_eq!(changed.len(), 1);
    assert_eq!(changed[0].0, e1);
    assert_eq!(changed[0].1.x, 99.0);
}

#[test]
fn query_changed_empty_when_no_mutations() {
    let mut engine = Engine::new();
    let _e = engine.world_mut().spawn((Pos { x: 1.0 },));

    engine.run_once(); // tick → 2

    // No mutations since tick 1
    let changed: Vec<_> = engine.world().query_changed::<Pos>(1);
    assert!(changed.is_empty());
}

#[test]
fn query_added_filters_by_tick() {
    let mut engine = Engine::new();
    let _e1 = engine.world_mut().spawn((Pos { x: 1.0 },));

    engine.run_once(); // tick → 2

    // e1 was added at tick 1. query_added(since: 1) should be empty.
    let added: Vec<_> = engine.world().query_added::<Pos>(1);
    assert!(added.is_empty());

    // Spawn e2 at tick 2
    let e2 = engine.world_mut().spawn((Pos { x: 2.0 },));

    // query_added(since: 1) should return only e2
    let added: Vec<_> = engine.world().query_added::<Pos>(1);
    assert_eq!(added.len(), 1);
    assert_eq!(added[0].0, e2);
}

#[test]
fn mutation_does_not_change_added_tick() {
    let mut engine = Engine::new();
    let e = engine.world_mut().spawn((Pos { x: 0.0 },));

    // Added at tick 1
    assert_eq!(engine.world().component_added_tick::<Pos>(e), Some(1));

    engine.run_once(); // tick → 2
    engine.world_mut().get_mut::<Pos>(e).unwrap().x = 99.0;

    // added_tick should still be 1 (mutation only updates changed_tick)
    assert_eq!(engine.world().component_added_tick::<Pos>(e), Some(1));
    assert_eq!(engine.world().component_changed_tick::<Pos>(e), Some(2));
}

#[test]
fn query_added_since_zero_returns_all() {
    let mut world = World::new();
    world.spawn((Pos { x: 1.0 },));
    world.spawn((Pos { x: 2.0 },));

    let added: Vec<_> = world.query_added::<Pos>(0);
    assert_eq!(added.len(), 2);
}

#[test]
fn despawned_entity_tick_returns_none() {
    let mut world = World::new();
    let e = world.spawn((Pos { x: 1.0 },));

    assert_eq!(world.component_added_tick::<Pos>(e), Some(1));
    assert_eq!(world.component_changed_tick::<Pos>(e), Some(1));

    world.despawn(e);

    // After despawn, tick queries should return None
    assert_eq!(world.component_added_tick::<Pos>(e), None);
    assert_eq!(world.component_changed_tick::<Pos>(e), None);
}

#[test]
fn overwrite_preserves_added_tick() {
    let mut engine = Engine::new();
    let e = engine.world_mut().spawn((Tag(1),));

    // Added at tick 1
    assert_eq!(engine.world().component_added_tick::<Tag>(e), Some(1));
    assert_eq!(engine.world().component_changed_tick::<Tag>(e), Some(1));

    engine.run_once(); // tick → 2

    // Mutate via get_mut at tick 2
    engine.world_mut().get_mut::<Tag>(e).unwrap().0 = 99;

    // added_tick preserved, changed_tick updated
    assert_eq!(engine.world().component_added_tick::<Tag>(e), Some(1));
    assert_eq!(engine.world().component_changed_tick::<Tag>(e), Some(2));
}

#[test]
fn query_changed_unregistered_type_returns_empty() {
    let world = World::new();
    let changed: Vec<_> = world.query_changed::<Unregistered>(0);
    assert!(changed.is_empty());

    let added: Vec<_> = world.query_added::<Unregistered>(0);
    assert!(added.is_empty());
}
