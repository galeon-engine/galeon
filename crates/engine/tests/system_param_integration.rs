// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

use galeon_engine::{Component, Engine, Query, QueryMut, Res, ResMut, World};

#[derive(Component, Debug)]
struct Pos {
    x: f32,
    y: f32,
}

#[derive(Component, Debug)]
struct Vel {
    x: f32,
    y: f32,
}

struct DeltaTime(f64);

fn movement(mut positions: QueryMut<'_, Pos>, velocities: Query<'_, Vel>, dt: Res<'_, DeltaTime>) {
    let _ = velocities.len();
    let _ = dt.0;
    for (_, pos) in positions.iter_mut() {
        pos.x += 1.0;
    }
}

#[test]
fn movement_system_end_to_end() {
    let mut engine = Engine::new();
    engine.insert_resource(DeltaTime(0.016));
    engine
        .world_mut()
        .spawn((Pos { x: 0.0, y: 0.0 }, Vel { x: 1.0, y: 0.0 }));
    engine.add_system::<(QueryMut<'_, Pos>, Query<'_, Vel>, Res<'_, DeltaTime>)>(
        "simulate", "movement", movement,
    );
    engine.run_once();
    let positions: Vec<f32> = engine
        .world()
        .query::<Pos>()
        .iter()
        .map(|(_, p)| p.x)
        .collect();
    assert_eq!(positions, vec![1.0]);
}

fn apply_gravity(mut vels: QueryMut<'_, Vel>, dt: Res<'_, DeltaTime>) {
    for (_, vel) in vels.iter_mut() {
        vel.y -= 9.8 * dt.0 as f32;
    }
}

fn apply_velocity(mut positions: QueryMut<'_, Pos>) {
    for (_, pos) in positions.iter_mut() {
        pos.y += 1.0;
    }
}

#[test]
fn multiple_param_systems_in_sequence() {
    let mut engine = Engine::new();
    engine.insert_resource(DeltaTime(1.0));
    engine
        .world_mut()
        .spawn((Pos { x: 0.0, y: 100.0 }, Vel { x: 0.0, y: 0.0 }));
    engine.add_system::<(QueryMut<'_, Vel>, Res<'_, DeltaTime>)>(
        "simulate",
        "gravity",
        apply_gravity,
    );
    engine.add_system::<(QueryMut<'_, Pos>,)>("simulate", "velocity", apply_velocity);
    engine.run_once();
    let vel_y = engine.world().query::<Vel>()[0].1.y;
    assert!((vel_y - (-9.8)).abs() < 0.01);
}

fn legacy_system(world: &mut World) {
    for (_, pos) in world.query_mut::<Pos>() {
        pos.x += 100.0;
    }
}

#[test]
fn legacy_and_param_systems_coexist() {
    let mut engine = Engine::new();
    engine.insert_resource(DeltaTime(1.0));
    engine
        .world_mut()
        .spawn((Pos { x: 0.0, y: 0.0 }, Vel { x: 0.0, y: 0.0 }));
    engine.add_system::<()>("pre", "legacy", legacy_system as fn(&mut World));
    engine.add_system::<(QueryMut<'_, Vel>, Res<'_, DeltaTime>)>(
        "simulate",
        "gravity",
        apply_gravity,
    );
    engine.run_once();
    let pos_x = engine.world().query::<Pos>()[0].1.x;
    assert!((pos_x - 100.0).abs() < 0.01);
}

fn conflicting(_a: Res<'_, DeltaTime>, _b: ResMut<'_, DeltaTime>) {}

#[test]
#[should_panic(expected = "conflicting parameter access")]
fn intra_system_conflict_panics() {
    let mut engine = Engine::new();
    engine.add_system::<(Res<'_, DeltaTime>, ResMut<'_, DeltaTime>)>(
        "update",
        "conflict",
        conflicting,
    );
}

fn double_dt(mut dt: ResMut<'_, DeltaTime>) {
    dt.0 *= 2.0;
}

#[test]
fn res_mut_persists_between_ticks() {
    let mut engine = Engine::new();
    engine.insert_resource(DeltaTime(1.0));
    engine.add_system::<(ResMut<'_, DeltaTime>,)>("update", "double_dt", double_dt);
    engine.run_once();
    assert!((engine.world().resource::<DeltaTime>().0 - 2.0).abs() < 0.01);
    engine.run_once();
    assert!((engine.world().resource::<DeltaTime>().0 - 4.0).abs() < 0.01);
}
