// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

//! Integration tests for protocol manifest generation (#47).

use galeon_engine::manifest::ProtocolManifest;
use galeon_engine::protocol::ProtocolKind;

// --- Define sample protocol items ---

/// Dispatch a ship to a contract destination.
#[galeon_engine::command]
pub struct DispatchShip {
    pub ship_id: u64,
    pub contract_id: u64,
}

/// Get the current fleet snapshot.
#[galeon_engine::query]
pub struct GetFleetSnapshot;

/// A ship has arrived at its destination.
#[galeon_engine::event]
pub struct ShipArrived {
    pub ship_id: u64,
    pub arrived_at: u64,
}

/// Snapshot of a single ship's view data.
#[galeon_engine::dto]
pub struct ShipView {
    pub ship_id: u64,
    pub name: String,
}

// --- Tests ---

#[test]
fn manifest_collects_all_items() {
    let manifest = ProtocolManifest::collect("test-protocol@0.1");

    assert!(!manifest.commands.is_empty(), "should have commands");
    assert!(!manifest.queries.is_empty(), "should have queries");
    assert!(!manifest.events.is_empty(), "should have events");
    assert!(!manifest.dtos.is_empty(), "should have dtos");
}

#[test]
fn manifest_has_correct_versions() {
    let manifest = ProtocolManifest::collect("moonbarons-protocol@0.1");

    assert_eq!(manifest.manifest_version, "1");
    assert_eq!(manifest.protocol_version, "moonbarons-protocol@0.1");
}

#[test]
fn manifest_command_entry_has_fields() {
    let manifest = ProtocolManifest::collect("test@0.1");

    let cmd = manifest
        .commands
        .iter()
        .find(|e| e.name == "DispatchShip")
        .expect("DispatchShip should be in commands");

    assert_eq!(cmd.kind, ProtocolKind::Command);
    assert_eq!(cmd.fields.len(), 2);
    assert_eq!(cmd.fields[0].name, "ship_id");
    assert_eq!(cmd.fields[0].ty, "u64");
    assert_eq!(cmd.fields[1].name, "contract_id");
    assert_eq!(cmd.fields[1].ty, "u64");
}

#[test]
fn manifest_query_unit_struct() {
    let manifest = ProtocolManifest::collect("test@0.1");

    let q = manifest
        .queries
        .iter()
        .find(|e| e.name == "GetFleetSnapshot")
        .expect("GetFleetSnapshot should be in queries");

    assert_eq!(q.kind, ProtocolKind::Query);
    assert!(q.fields.is_empty(), "unit struct should have no fields");
}

#[test]
fn manifest_event_entry_has_fields() {
    let manifest = ProtocolManifest::collect("test@0.1");

    let evt = manifest
        .events
        .iter()
        .find(|e| e.name == "ShipArrived")
        .expect("ShipArrived should be in events");

    assert_eq!(evt.kind, ProtocolKind::Event);
    assert_eq!(evt.fields.len(), 2);
}

#[test]
fn manifest_dto_entry_has_fields() {
    let manifest = ProtocolManifest::collect("test@0.1");

    let dto = manifest
        .dtos
        .iter()
        .find(|e| e.name == "ShipView")
        .expect("ShipView should be in dtos");

    assert_eq!(dto.kind, ProtocolKind::Dto);
    assert_eq!(dto.fields.len(), 2);
    assert_eq!(dto.fields[0].name, "ship_id");
    assert_eq!(dto.fields[1].name, "name");
    assert_eq!(dto.fields[1].ty, "String");
}

#[test]
fn manifest_doc_comments_captured() {
    let manifest = ProtocolManifest::collect("test@0.1");

    let cmd = manifest
        .commands
        .iter()
        .find(|e| e.name == "DispatchShip")
        .expect("DispatchShip should be in commands");

    assert!(
        cmd.doc.contains("Dispatch a ship"),
        "doc should be captured: got {:?}",
        cmd.doc
    );
}

#[test]
fn manifest_json_roundtrip() {
    let manifest = ProtocolManifest::collect("test@0.1");
    let json = manifest.to_json_pretty().unwrap();

    // Verify it's valid JSON that round-trips.
    let back: ProtocolManifest = serde_json::from_str(&json).unwrap();
    assert_eq!(back.manifest_version, "1");
    assert_eq!(back.protocol_version, "test@0.1");

    // Verify pretty-printed and human-readable.
    assert!(json.contains('\n'), "should be pretty-printed");
    assert!(json.contains("DispatchShip"));
}

#[test]
fn manifest_ron_roundtrip() {
    let manifest = ProtocolManifest::collect("test@0.1");
    let ron_str = manifest.to_ron_pretty().unwrap();

    let back: ProtocolManifest = ron::from_str(&ron_str).unwrap();
    assert_eq!(back.manifest_version, "1");
    assert_eq!(back.protocol_version, "test@0.1");
}
