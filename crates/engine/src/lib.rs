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
pub mod handler_function;
pub mod manifest;
pub mod particle;
pub mod protocol;
pub mod query;
pub mod render;
pub mod render_channel;
pub mod render_event;
mod resource;
pub mod route_scanner;
pub mod schedule;
pub mod selection;
pub mod system_param;
pub mod virtual_time;
pub mod world;

// Re-export dependencies that macros reference so consumers only need
// `galeon-engine` in their Cargo.toml.
#[doc(hidden)]
pub use inventory;
#[doc(hidden)]
pub use serde;
#[doc(hidden)]
pub use serde_json;

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
pub use galeon_engine_macros::{Component, command, dto, event, handler, query};
pub use game_loop::FixedTimestep;
pub use handler::HandlerRegistry;
pub use handler_function::{
    Handler, IntoHandler, run_handler, run_json_handler, run_json_handler_function,
    run_json_handler_value,
};
pub use manifest::{
    FieldEntry, HandlerRegistration, ManifestEntry, ManifestField, ProtocolManifest,
    ProtocolRegistration,
};
pub use particle::{
    Billboard, ColorDist, Emitter, FloatDist, Particle, ParticleRng, Vec3Dist,
    emitter_spawn_expire_system,
};
pub use protocol::{Command, Dto, Event, ProtocolKind, ProtocolMeta, ProtocolQuery};
pub use query::{
    AddedIter, ChangedIter, Mut, NoFilter, Query2Iter, Query2MutIter, Query3Iter, Query3MutIter,
    QueryFilter, QueryIter, QueryIterMut, QuerySpec, QuerySpecMut, With, Without,
};
pub use render::{
    InstanceOf, MaterialHandle, MeshHandle, ObjectType, ParentEntity, Tint, Transform, Visibility,
};
pub use render_channel::{ChannelRegistration, ExtractToFloats, RenderChannelRegistry};
pub use render_event::{FrameEvent, RenderEvent, RenderEventRegistry};
pub use route_scanner::{
    HandlerMeta, ResolvedRoute, ScannedRoute, crate_relative_handler_fn_path, generate_axum_routes,
    resolve_routes, scan_api_routes, strip_type_prefix,
};
pub use schedule::Schedule;
pub use selection::{PickModifiers, PickPoint, Selection};
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
