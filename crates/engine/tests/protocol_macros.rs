// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

//! Integration tests for protocol attribute macros (#46).

use galeon_engine::protocol::{ProtocolKind, ProtocolMeta};

// --- Compile-pass: all four macro kinds ---

#[galeon_engine::command]
pub struct DispatchShip {
    pub ship_id: u64,
    pub contract_id: u64,
}

#[galeon_engine::query]
pub struct GetFleetSnapshot;

#[galeon_engine::event]
pub struct ShipArrived {
    pub ship_id: u64,
    pub arrived_at: u64,
}

#[galeon_engine::dto]
pub struct FleetSnapshot {
    pub ship_count: u32,
}

// --- ProtocolMeta correctness ---

#[test]
fn command_meta() {
    assert_eq!(DispatchShip::name(), "DispatchShip");
    assert_eq!(DispatchShip::kind(), ProtocolKind::Command);
}

#[test]
fn query_meta() {
    assert_eq!(GetFleetSnapshot::name(), "GetFleetSnapshot");
    assert_eq!(GetFleetSnapshot::kind(), ProtocolKind::Query);
}

#[test]
fn event_meta() {
    assert_eq!(ShipArrived::name(), "ShipArrived");
    assert_eq!(ShipArrived::kind(), ProtocolKind::Event);
}

#[test]
fn dto_meta() {
    assert_eq!(FleetSnapshot::name(), "FleetSnapshot");
    assert_eq!(FleetSnapshot::kind(), ProtocolKind::Dto);
}

// --- Serde round-trip ---

#[test]
fn command_serde_roundtrip() {
    let cmd = DispatchShip {
        ship_id: 1,
        contract_id: 42,
    };
    let json = serde_json::to_string(&cmd).unwrap();
    let back: DispatchShip = serde_json::from_str(&json).unwrap();
    assert_eq!(back.ship_id, 1);
    assert_eq!(back.contract_id, 42);
}

#[test]
fn query_serde_roundtrip() {
    let q = GetFleetSnapshot;
    let json = serde_json::to_string(&q).unwrap();
    let _back: GetFleetSnapshot = serde_json::from_str(&json).unwrap();
    let _ = q; // silence unused warning
}

#[test]
fn event_serde_roundtrip() {
    let evt = ShipArrived {
        ship_id: 7,
        arrived_at: 9999,
    };
    let json = serde_json::to_string(&evt).unwrap();
    let back: ShipArrived = serde_json::from_str(&json).unwrap();
    assert_eq!(back.ship_id, 7);
}

#[test]
fn dto_serde_roundtrip() {
    let dto = FleetSnapshot { ship_count: 42 };
    let json = serde_json::to_string(&dto).unwrap();
    let back: FleetSnapshot = serde_json::from_str(&json).unwrap();
    assert_eq!(back.ship_count, 42);
}

// --- Dto gets Clone ---

#[test]
fn dto_is_clone() {
    let dto = FleetSnapshot { ship_count: 10 };
    let cloned = dto.clone();
    assert_eq!(cloned.ship_count, 10);
}

// --- Marker traits are implemented ---

fn _assert_command<T: galeon_engine::protocol::Command>() {}
fn _assert_query<T: galeon_engine::protocol::Query>() {}
fn _assert_event<T: galeon_engine::protocol::Event>() {}
fn _assert_dto<T: galeon_engine::protocol::Dto>() {}

#[test]
fn marker_traits_are_implemented() {
    _assert_command::<DispatchShip>();
    _assert_query::<GetFleetSnapshot>();
    _assert_event::<ShipArrived>();
    _assert_dto::<FleetSnapshot>();
}
