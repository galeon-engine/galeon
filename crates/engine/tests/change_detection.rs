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
