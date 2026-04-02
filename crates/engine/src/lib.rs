// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

#![allow(private_interfaces)]

// Allow derive macros to reference `galeon_engine::component::Component`
// when used within this crate's own tests.
extern crate self as galeon_engine;

pub mod archetype;
pub mod codegen;
pub mod commands;
pub mod component;
pub mod data;
pub mod deadline;
pub mod engine;
pub mod entity;
pub mod event;
pub mod function_system;
pub mod game_loop;
pub mod handler;
pub mod manifest;
pub mod protocol;
pub mod query;
pub mod render;
pub mod render_channel;
mod resource;
pub mod schedule;
pub mod system_param;
pub mod virtual_time;
pub mod world;

// Re-export dependencies that macros reference so consumers only need
// `galeon-engine` in their Cargo.toml.
#[doc(hidden)]
pub use inventory;
#[doc(hidden)]
pub use serde;

// Re-exports for ergonomic API.
pub use codegen::{generate_descriptors, generate_typescript};
pub use commands::Commands;
pub use component::Component;
pub use data::{DataRegistry, UnitStats, UnitTemplate};
pub use deadline::{Clock, DeadlineId, Deadlines, SystemClock, TestClock, Timestamp};
pub use engine::{Engine, Plugin};
pub use entity::Entity;
pub use event::{EventReader, EventWriter, Events};
pub use function_system::{IntoSystem, System};
pub use galeon_engine_macros::{Component, command, dto, event, query};
pub use game_loop::FixedTimestep;
pub use handler::HandlerRegistry;
pub use manifest::{
    FieldEntry, ManifestEntry, ManifestField, ProtocolManifest, ProtocolRegistration,
};
pub use protocol::{Command, Dto, Event, ProtocolKind, ProtocolMeta, ProtocolQuery};
pub use query::{
    NoFilter, Query2Iter, Query2MutIter, Query3Iter, Query3MutIter, QueryFilter, QueryIter,
    QueryIterMut, QuerySpec, QuerySpecMut, With, Without,
};
pub use render::{MaterialHandle, MeshHandle, Transform, Visibility};
pub use render_channel::{ChannelRegistration, ExtractToFloats, RenderChannelRegistry};
pub use schedule::Schedule;
pub use system_param::{Access, Query, QueryMut, Res, ResMut, SystemParam};
pub use virtual_time::VirtualTime;
pub use world::{UnsafeWorldCell, World};

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
