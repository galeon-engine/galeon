// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

//! Integration tests for protocol attribute macros (#46).

use galeon_engine::protocol::{ProtocolKind, ProtocolMeta};

// --- Compile-pass: all four macro kinds ---

#[galeon_engine::command]
pub struct SpawnUnit {
    pub unit_id: u64,
    pub location_id: u64,
}

#[galeon_engine::query]
pub struct GetWorldSnapshot;

#[galeon_engine::event]
pub struct UnitDestroyed {
    pub unit_id: u64,
    pub arrived_at: u64,
}

#[galeon_engine::dto]
pub struct WorldSnapshot {
    pub unit_count: u32,
}

// --- ProtocolMeta correctness ---

#[test]
fn command_meta() {
    assert_eq!(SpawnUnit::name(), "SpawnUnit");
    assert_eq!(SpawnUnit::kind(), ProtocolKind::Command);
}

#[test]
fn query_meta() {
    assert_eq!(GetWorldSnapshot::name(), "GetWorldSnapshot");
    assert_eq!(GetWorldSnapshot::kind(), ProtocolKind::Query);
}

#[test]
fn event_meta() {
    assert_eq!(UnitDestroyed::name(), "UnitDestroyed");
    assert_eq!(UnitDestroyed::kind(), ProtocolKind::Event);
}

#[test]
fn dto_meta() {
    assert_eq!(WorldSnapshot::name(), "WorldSnapshot");
    assert_eq!(WorldSnapshot::kind(), ProtocolKind::Dto);
}

// --- Serde round-trip ---

#[test]
fn command_serde_roundtrip() {
    let cmd = SpawnUnit {
        unit_id: 1,
        location_id: 42,
    };
    let json = serde_json::to_string(&cmd).unwrap();
    let back: SpawnUnit = serde_json::from_str(&json).unwrap();
    assert_eq!(back.unit_id, 1);
    assert_eq!(back.location_id, 42);
}

#[test]
fn query_serde_roundtrip() {
    let q = GetWorldSnapshot;
    let json = serde_json::to_string(&q).unwrap();
    let _back: GetWorldSnapshot = serde_json::from_str(&json).unwrap();
    let _ = q; // silence unused warning
}

#[test]
fn event_serde_roundtrip() {
    let evt = UnitDestroyed {
        unit_id: 7,
        arrived_at: 9999,
    };
    let json = serde_json::to_string(&evt).unwrap();
    let back: UnitDestroyed = serde_json::from_str(&json).unwrap();
    assert_eq!(back.unit_id, 7);
}

#[test]
fn dto_serde_roundtrip() {
    let dto = WorldSnapshot { unit_count: 42 };
    let json = serde_json::to_string(&dto).unwrap();
    let back: WorldSnapshot = serde_json::from_str(&json).unwrap();
    assert_eq!(back.unit_count, 42);
}

// --- Dto gets Clone ---

#[test]
fn dto_is_clone() {
    let dto = WorldSnapshot { unit_count: 10 };
    let cloned = dto.clone();
    assert_eq!(cloned.unit_count, 10);
}

// --- Marker traits are implemented ---

fn _assert_command<T: galeon_engine::protocol::Command>() {}
fn _assert_query<T: galeon_engine::protocol::ProtocolQuery>() {}
fn _assert_event<T: galeon_engine::protocol::Event>() {}
fn _assert_dto<T: galeon_engine::protocol::Dto>() {}

#[test]
fn marker_traits_are_implemented() {
    _assert_command::<SpawnUnit>();
    _assert_query::<GetWorldSnapshot>();
    _assert_event::<UnitDestroyed>();
    _assert_dto::<WorldSnapshot>();
}
