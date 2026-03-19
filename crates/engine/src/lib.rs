// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

#![allow(private_interfaces)]

// Allow derive macros to reference `galeon_engine::component::Component`
// when used within this crate's own tests.
extern crate self as galeon_engine;

pub mod component;
pub mod data;
pub mod engine;
pub mod entity;
pub mod game_loop;
pub mod render;
mod resource;
pub mod schedule;
pub mod tick;
pub mod world;

// Re-exports for ergonomic API.
pub use component::Component;
pub use data::{DataRegistry, UnitStats, UnitTemplate};
pub use engine::{Engine, Plugin};
pub use entity::Entity;
pub use galeon_engine_macros::Component;
pub use game_loop::FixedTimestep;
pub use render::{MaterialHandle, MeshHandle, Transform, Visibility};
pub use schedule::Schedule;
pub use world::World;

/// Returns the engine version string.
pub fn engine_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_not_empty() {
        assert!(!engine_version().is_empty());
    }

    #[test]
    fn derive_component_compiles() {
        #[derive(Component)]
        struct Position {
            _x: f32,
            _y: f32,
        }

        let _pos = Position { _x: 0.0, _y: 0.0 };
    }
}
