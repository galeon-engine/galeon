// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

//! Protocol marker traits and metadata for Galeon's boundary abstraction.
//!
//! The protocol layer defines four concepts that let the same game logic work
//! in-process, over HTTP/WS, or through native bindings:
//!
//! - [`Command`] — state-changing requests
//! - [`ProtocolQuery`] — read-only requests
//! - [`Event`] — authoritative facts emitted after state transitions
//! - [`Dto`] — boundary-facing data structures
//!
//! Each protocol item can implement [`ProtocolMeta`] to expose its name and
//! [`ProtocolKind`] for manifest generation and codegen tooling.
//!
//! # Design
//!
//! These traits define *vocabulary only*. They carry no transport semantics,
//! no manifest emission, and no runtime behavior. Attribute macros in
//! `galeon-engine-macros` emit concrete [`ProtocolMeta`] implementations per
//! annotated item — see issue #46.

use serde::{Deserialize, Serialize};

/// Discriminant for protocol item kinds.
///
/// Used by [`ProtocolMeta`] to identify what role a protocol item plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProtocolKind {
    /// A state-changing request.
    Command,
    /// A read-only request.
    Query,
    /// An authoritative fact emitted after a state transition.
    Event,
    /// A boundary-facing data structure.
    Dto,
}

/// Metadata trait for protocol items.
///
/// Provides the item's stable name and [`ProtocolKind`] discriminant.
/// This is the smallest surface macros need to target for manifest
/// generation (#47).
///
/// # Implementors
///
/// Do not implement this trait manually in production code. Use the
/// `#[galeon::command]`, `#[galeon::query]`, `#[galeon::event]`, or
/// `#[galeon::dto]` attribute macros, which emit one concrete impl per
/// annotated item.
///
/// Manual implementation is valid for testing and for types that cannot
/// use the attribute macros.
pub trait ProtocolMeta {
    /// The stable protocol name for this item.
    fn name() -> &'static str;

    /// The protocol kind discriminant.
    fn kind() -> ProtocolKind;
}

/// Marker trait for state-changing requests.
///
/// Commands represent intent to mutate game state. They travel from client
/// to server and are validated before execution.
///
/// # Bounds
///
/// Requires `Serialize + Deserialize + Send + Sync + 'static` so commands
/// can cross thread and serialization boundaries.
pub trait Command: Serialize + for<'de> Deserialize<'de> + Send + Sync + 'static {}

/// Marker trait for read-only requests.
///
/// Queries request a snapshot of game state without side effects.
///
/// # Bounds
///
/// Same as [`Command`]: `Serialize + Deserialize + Send + Sync + 'static`.
///
/// Renamed from `Query` to `ProtocolQuery` in #57 to free up the `Query` name
/// for the far more frequently used ECS system parameter.
pub trait ProtocolQuery: Serialize + for<'de> Deserialize<'de> + Send + Sync + 'static {}

/// Marker trait for authoritative facts emitted after state transitions.
///
/// Events are immutable records of something that happened. They flow from
/// server to client (and potentially to other server-side subscribers).
///
/// # Bounds
///
/// Same as [`Command`]: `Serialize + Deserialize + Send + Sync + 'static`.
pub trait Event: Serialize + for<'de> Deserialize<'de> + Send + Sync + 'static {}

/// Marker trait for boundary-facing data structures.
///
/// DTOs are snapshot/view types that get copied across boundaries. They
/// carry no behavior — only data.
///
/// # Bounds
///
/// Adds `Clone` on top of the standard protocol bounds because DTOs are
/// value types that get copied freely.
pub trait Dto: Serialize + for<'de> Deserialize<'de> + Clone + Send + Sync + 'static {}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Sample structs for each trait ---

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct SpawnUnit {
        unit_id: u64,
        location_id: u64,
    }
    impl Command for SpawnUnit {}
    impl ProtocolMeta for SpawnUnit {
        fn name() -> &'static str {
            "SpawnUnit"
        }
        fn kind() -> ProtocolKind {
            ProtocolKind::Command
        }
    }

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct GetWorldSnapshot;
    impl ProtocolQuery for GetWorldSnapshot {}
    impl ProtocolMeta for GetWorldSnapshot {
        fn name() -> &'static str {
            "GetWorldSnapshot"
        }
        fn kind() -> ProtocolKind {
            ProtocolKind::Query
        }
    }

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct UnitDestroyed {
        unit_id: u64,
        arrived_at: u64,
    }
    impl Event for UnitDestroyed {}
    impl ProtocolMeta for UnitDestroyed {
        fn name() -> &'static str {
            "UnitDestroyed"
        }
        fn kind() -> ProtocolKind {
            ProtocolKind::Event
        }
    }

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    struct WorldSnapshot {
        unit_count: u32,
    }
    impl Dto for WorldSnapshot {}
    impl ProtocolMeta for WorldSnapshot {
        fn name() -> &'static str {
            "WorldSnapshot"
        }
        fn kind() -> ProtocolKind {
            ProtocolKind::Dto
        }
    }

    // --- T6: ProtocolKind ---

    #[test]
    fn protocol_kind_variants() {
        assert_ne!(ProtocolKind::Command, ProtocolKind::Query);
        assert_ne!(ProtocolKind::Event, ProtocolKind::Dto);
    }

    #[test]
    fn protocol_kind_serde_roundtrip() {
        for kind in [
            ProtocolKind::Command,
            ProtocolKind::Query,
            ProtocolKind::Event,
            ProtocolKind::Dto,
        ] {
            let json = serde_json::to_string(&kind).unwrap();
            let back: ProtocolKind = serde_json::from_str(&json).unwrap();
            assert_eq!(kind, back);
        }
    }

    // --- T5: ProtocolMeta returns correct kind ---

    #[test]
    fn protocol_meta_command() {
        assert_eq!(SpawnUnit::name(), "SpawnUnit");
        assert_eq!(SpawnUnit::kind(), ProtocolKind::Command);
    }

    #[test]
    fn protocol_meta_query() {
        assert_eq!(GetWorldSnapshot::name(), "GetWorldSnapshot");
        assert_eq!(GetWorldSnapshot::kind(), ProtocolKind::Query);
    }

    #[test]
    fn protocol_meta_event() {
        assert_eq!(UnitDestroyed::name(), "UnitDestroyed");
        assert_eq!(UnitDestroyed::kind(), ProtocolKind::Event);
    }

    #[test]
    fn protocol_meta_dto() {
        assert_eq!(WorldSnapshot::name(), "WorldSnapshot");
        assert_eq!(WorldSnapshot::kind(), ProtocolKind::Dto);
    }

    // --- T1-T4: Serde round-trip ---

    #[test]
    fn command_serde_roundtrip() {
        let cmd = SpawnUnit {
            unit_id: 1,
            location_id: 42,
        };
        let json = serde_json::to_string(&cmd).unwrap();
        let back: SpawnUnit = serde_json::from_str(&json).unwrap();
        assert_eq!(cmd, back);
    }

    #[test]
    fn query_serde_roundtrip() {
        let q = GetWorldSnapshot;
        let json = serde_json::to_string(&q).unwrap();
        let back: GetWorldSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(q, back);
    }

    #[test]
    fn event_serde_roundtrip() {
        let evt = UnitDestroyed {
            unit_id: 1,
            arrived_at: 1000,
        };
        let json = serde_json::to_string(&evt).unwrap();
        let back: UnitDestroyed = serde_json::from_str(&json).unwrap();
        assert_eq!(evt, back);
    }

    #[test]
    fn dto_serde_roundtrip() {
        let dto = WorldSnapshot { unit_count: 5 };
        let json = serde_json::to_string(&dto).unwrap();
        let back: WorldSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(dto, back);
    }

    // --- Acceptance: Component + Command coexistence ---

    #[test]
    fn component_and_command_no_conflict() {
        use crate::component::Component;

        #[derive(Debug, Serialize, Deserialize)]
        struct Health {
            hp: u32,
        }
        impl Component for Health {}
        impl Command for Health {}
        impl ProtocolMeta for Health {
            fn name() -> &'static str {
                "Health"
            }
            fn kind() -> ProtocolKind {
                ProtocolKind::Command
            }
        }

        // Both traits coexist without conflict.
        let h = Health { hp: 100 };
        assert_eq!(Health::kind(), ProtocolKind::Command);
        let json = serde_json::to_string(&h).unwrap();
        assert!(json.contains("100"));
    }
}
