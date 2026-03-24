// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

#![allow(private_interfaces)]

// Allow derive macros to reference `galeon_engine::component::Component`
// when used within this crate's own tests.
extern crate self as galeon_engine;

pub mod archetype;
pub mod component;
pub mod data;
pub mod engine;
pub mod entity;
pub mod game_loop;
pub mod manifest;
pub mod protocol;
pub mod query;
pub mod render;
mod resource;
pub mod schedule;
pub mod virtual_time;
pub mod world;

// Re-export dependencies that macros reference so consumers only need
// `galeon-engine` in their Cargo.toml.
#[doc(hidden)]
pub use inventory;
#[doc(hidden)]
pub use serde;

// Re-exports for ergonomic API.
pub use component::Component;
pub use data::{DataRegistry, UnitStats, UnitTemplate};
pub use engine::{Engine, Plugin};
pub use entity::Entity;
pub use galeon_engine_macros::{Component, command, dto, event, query};
pub use game_loop::FixedTimestep;
pub use manifest::{
    FieldEntry, ManifestEntry, ManifestField, ProtocolManifest, ProtocolRegistration,
};
pub use protocol::{Command, Dto, Event, ProtocolKind, ProtocolMeta, Query};
pub use query::{
    NoFilter, QueryFilter, QueryIter, QueryIterMut, QuerySpec, QuerySpecMut, With, Without,
};
pub use render::{MaterialHandle, MeshHandle, Transform, Visibility};
pub use schedule::Schedule;
pub use virtual_time::VirtualTime;
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
