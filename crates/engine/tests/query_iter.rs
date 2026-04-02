// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

use galeon_engine::{Component, With, Without, World};

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

#[derive(Component, Debug, Clone, PartialEq)]
struct Health(i32);

#[test]
fn query_yields_matching_entities() {
    let mut world = World::new();
    world.spawn((Pos { x: 1.0, y: 0.0 },));
    world.spawn((Pos { x: 2.0, y: 0.0 }, Vel { dx: 0.0, dy: 0.0 }));
    world.spawn((Vel { dx: 3.0, dy: 0.0 },)); // no Pos

    let positions: Vec<f32> = world.query::<&Pos>().map(|(_, p)| p.x).collect();
    assert_eq!(positions.len(), 2);
    assert!(positions.contains(&1.0));
    assert!(positions.contains(&2.0));
}

#[test]
fn query_empty_world() {
    let world = World::new();
    assert!(world.query::<&Pos>().next().is_none());
}

#[test]
fn query_skips_despawned_entities() {
    let mut world = World::new();
    let e1 = world.spawn((Pos { x: 1.0, y: 0.0 },));
    let _e2 = world.spawn((Pos { x: 2.0, y: 0.0 },));
    world.despawn(e1);

    let results: Vec<_> = world.query::<&Pos>().collect();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].1.x, 2.0);
}

#[test]
fn query_entity_is_copy_not_ref() {
    let mut world = World::new();
    let e = world.spawn((Pos { x: 5.0, y: 0.0 },));

    let results: Vec<_> = world.query::<&Pos>().collect();
    let (entity, pos) = &results[0];
    assert_eq!(*entity, e);
    assert_eq!(pos.x, 5.0);
}

#[test]
fn query_mut_allows_modification() {
    let mut world = World::new();
    world.spawn((Pos { x: 0.0, y: 0.0 },));
    world.spawn((Pos { x: 10.0, y: 10.0 },));

    for (_, pos) in world.query_mut::<&mut Pos>() {
        pos.x += 1.0;
    }

    let xs: Vec<f32> = world.query::<&Pos>().map(|(_, p)| p.x).collect();
    assert!(xs.contains(&1.0));
    assert!(xs.contains(&11.0));
}

#[test]
fn query_mut_empty_world() {
    let mut world = World::new();
    assert!(world.query_mut::<&mut Pos>().next().is_none());
}

#[test]
fn query2_yields_entities_with_both() {
    let mut world = World::new();
    world.spawn((Pos { x: 1.0, y: 0.0 },));
    world.spawn((Pos { x: 2.0, y: 0.0 }, Vel { dx: 5.0, dy: 0.0 }));
    world.spawn((Vel { dx: 3.0, dy: 0.0 },));

    let results: Vec<_> = world.query::<(&Pos, &Vel)>().collect();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].1.0.x, 2.0);
    assert_eq!(results[0].1.1.dx, 5.0);
}

#[test]
fn query2_empty_when_no_overlap() {
    let mut world = World::new();
    world.spawn((Pos { x: 1.0, y: 0.0 },));
    world.spawn((Vel { dx: 2.0, dy: 0.0 },));

    assert!(world.query::<(&Pos, &Vel)>().next().is_none());
}

#[test]
fn query2_wrapper_matches_generic_query() {
    let mut world = World::new();
    world.spawn((Pos { x: 1.0, y: 0.0 }, Vel { dx: 5.0, dy: 0.0 }));
    world.spawn((Pos { x: 2.0, y: 0.0 },));

    let generic: Vec<_> = world.query::<(&Pos, &Vel)>().collect();
    let wrapper: Vec<_> = world.query2::<Pos, Vel>().collect();

    assert_eq!(generic.len(), wrapper.len());
    assert_eq!(generic[0].0, wrapper[0].0);
    assert_eq!(generic[0].1.0.x, wrapper[0].1.0.x);
    assert_eq!(generic[0].1.1.dx, wrapper[0].1.1.dx);
}

#[test]
fn query2_mut_mutates_both() {
    let mut world = World::new();
    let e = world.spawn((Pos { x: 1.0, y: 1.0 }, Vel { dx: 10.0, dy: 10.0 }));

    for (_, (pos, vel)) in world.query_mut::<(&mut Pos, &mut Vel)>() {
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

    let results: Vec<_> = world.query_mut::<(&mut Pos, &mut Vel)>().collect();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].0, e);
}

#[test]
#[should_panic(expected = "cannot borrow the same column mutably twice")]
fn query2_mut_same_type_panics() {
    let mut world = World::new();
    world.spawn((Pos { x: 0.0, y: 0.0 },));
    let _ = world.query_mut::<(&mut Pos, &mut Pos)>().next();
}

#[test]
#[should_panic(expected = "cannot borrow the same column mutably twice")]
fn pair_mut_optional_same_type_panics() {
    let mut world = World::new();
    world.spawn((Pos { x: 0.0, y: 0.0 },));
    let _ = world.query_mut::<(&mut Pos, Option<&mut Pos>)>().next();
}

#[test]
fn query3_wrapper_yields_entities_with_all_three() {
    let mut world = World::new();
    world.spawn((
        Pos { x: 1.0, y: 0.0 },
        Vel { dx: 2.0, dy: 0.0 },
        Health(100),
    ));
    world.spawn((Pos { x: 3.0, y: 0.0 }, Vel { dx: 4.0, dy: 0.0 }));
    world.spawn((Health(50),));

    let results: Vec<_> = world.query3::<Pos, Vel, Health>().collect();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].1.0.x, 1.0);
    assert_eq!(results[0].1.1.dx, 2.0);
    assert_eq!(results[0].1.2.0, 100);
}

#[test]
fn query3_mut_wrapper_mutates_all_three() {
    let mut world = World::new();
    let e = world.spawn((
        Pos { x: 1.0, y: 0.0 },
        Vel { dx: 2.0, dy: 0.0 },
        Health(100),
    ));

    for (_, (pos, vel, hp)) in world.query3_mut::<Pos, Vel, Health>() {
        pos.x += 10.0;
        vel.dx += 20.0;
        hp.0 -= 50;
    }

    assert_eq!(world.get::<Pos>(e).unwrap().x, 11.0);
    assert_eq!(world.get::<Vel>(e).unwrap().dx, 22.0);
    assert_eq!(world.get::<Health>(e).unwrap().0, 50);
}

#[test]
#[should_panic(expected = "cannot borrow the same column mutably twice")]
fn query3_mut_duplicate_type_panics() {
    let mut world = World::new();
    world.spawn((Pos { x: 0.0, y: 0.0 },));
    let _ = world.query3_mut::<Pos, Vel, Pos>();
}

#[test]
fn query_filtered_with_and_without() {
    let mut world = World::new();
    let e = world.spawn((Pos { x: 1.0, y: 0.0 }, Vel { dx: 2.0, dy: 0.0 }));
    world.spawn((Pos { x: 3.0, y: 0.0 },));
    world.spawn((Pos { x: 4.0, y: 0.0 }, Health(10)));

    let results: Vec<_> = world
        .query_filtered::<&Pos, (With<Vel>, Without<Health>)>()
        .collect();

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].0, e);
}

#[test]
fn query_iter_size_hint_tracks_remaining_rows() {
    let mut world = World::new();
    world.spawn((Pos { x: 1.0, y: 0.0 },));
    world.spawn((Pos { x: 2.0, y: 0.0 },));
    world.spawn((Pos { x: 3.0, y: 0.0 },));

    let mut iter = world.query::<&Pos>();
    assert_eq!(iter.size_hint(), (3, Some(3)));
    let _ = iter.next();
    assert_eq!(iter.size_hint(), (2, Some(2)));
    let _ = iter.next();
    assert_eq!(iter.size_hint(), (1, Some(1)));
}

#[test]
fn query_mut_size_hint_tracks_remaining_rows() {
    let mut world = World::new();
    world.spawn((Pos { x: 1.0, y: 0.0 }, Vel { dx: 1.0, dy: 0.0 }));
    world.spawn((Pos { x: 2.0, y: 0.0 }, Vel { dx: 2.0, dy: 0.0 }));

    let mut iter = world.query2_mut::<Pos, Vel>();
    assert_eq!(iter.size_hint(), (2, Some(2)));
    let _ = iter.next();
    assert_eq!(iter.size_hint(), (1, Some(1)));
}

#[test]
fn one_and_one_mut_use_typed_query_specs() {
    let mut world = World::new();
    let e = world.spawn((Pos { x: 1.0, y: 2.0 }, Vel { dx: 3.0, dy: 4.0 }));

    let (pos, vel) = world.one::<(&Pos, &Vel)>(e).unwrap();
    assert_eq!(pos.x, 1.0);
    assert_eq!(vel.dx, 3.0);

    let (pos, vel) = world.one_mut::<(&mut Pos, &mut Vel)>(e).unwrap();
    pos.x = 10.0;
    vel.dy = 20.0;

    assert_eq!(world.get::<Pos>(e).unwrap().x, 10.0);
    assert_eq!(world.get::<Vel>(e).unwrap().dy, 20.0);
}
