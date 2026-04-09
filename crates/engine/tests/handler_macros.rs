// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

//! Integration tests for the `#[handler]` attribute macro (#162).

use galeon_engine::manifest::HandlerRegistration;

// --- Sample protocol items (needed as request/response types) ---

#[galeon_engine::command]
pub struct DispatchFleet {
    pub fleet_id: u64,
    pub destination_id: u64,
}

#[galeon_engine::dto]
pub struct FleetStatus {
    pub fleet_id: u64,
    pub ok: bool,
}

#[galeon_engine::query]
pub struct GetFleetStatus {
    pub fleet_id: u64,
}

// --- Sample handlers ---

#[galeon_engine::handler]
pub fn dispatch_fleet(cmd: DispatchFleet) -> Result<FleetStatus, String> {
    Ok(FleetStatus {
        fleet_id: cmd.fleet_id,
        ok: true,
    })
}

#[galeon_engine::handler]
pub fn get_fleet_status(_query: GetFleetStatus) -> Result<FleetStatus, String> {
    Ok(FleetStatus {
        fleet_id: 0,
        ok: true,
    })
}

/// Handler with a real SystemParam extra parameter (Res).
/// Verifies that #[handler] accepts SystemParam extras that work with IntoHandler.
#[galeon_engine::handler]
pub fn handler_with_system_param(
    cmd: DispatchFleet,
    _config: galeon_engine::Res<'_, FleetConfig>,
) -> Result<FleetStatus, String> {
    Ok(FleetStatus {
        fleet_id: cmd.fleet_id,
        ok: true,
    })
}

/// Shared resource for handler SystemParam tests.
pub struct FleetConfig {
    pub max_fleet_size: u32,
}

// --- Tests ---

/// Collect all handler registrations and find ours by name.
fn find_registration(name: &str) -> Option<&'static HandlerRegistration> {
    inventory::iter::<HandlerRegistration>
        .into_iter()
        .find(|r| r.name == name)
}

#[test]
fn handler_registers_metadata() {
    let reg = find_registration("dispatch_fleet").expect("dispatch_fleet should be registered");
    assert_eq!(reg.name, "dispatch_fleet");
    assert_eq!(reg.request_type, "DispatchFleet");
    assert_eq!(reg.response_type, "FleetStatus");
    assert_eq!(reg.error_type, "String");
    assert!(!reg.module_path.is_empty());
}

#[test]
fn query_handler_registers_metadata() {
    let reg = find_registration("get_fleet_status").expect("get_fleet_status should be registered");
    assert_eq!(reg.name, "get_fleet_status");
    assert_eq!(reg.request_type, "GetFleetStatus");
    assert_eq!(reg.response_type, "FleetStatus");
    assert_eq!(reg.error_type, "String");
}

#[test]
fn handler_with_system_param_registers() {
    let reg = find_registration("handler_with_system_param")
        .expect("handler_with_system_param should be registered");
    assert_eq!(reg.request_type, "DispatchFleet");
}

#[test]
fn handler_collection_is_deterministic() {
    let names: Vec<&str> = {
        let mut v: Vec<&str> = inventory::iter::<HandlerRegistration>
            .into_iter()
            .map(|r| r.name)
            .collect();
        v.sort();
        v
    };
    let names2: Vec<&str> = {
        let mut v: Vec<&str> = inventory::iter::<HandlerRegistration>
            .into_iter()
            .map(|r| r.name)
            .collect();
        v.sort();
        v
    };
    assert_eq!(names, names2);
}

#[test]
fn handler_function_still_callable() {
    let result = dispatch_fleet(DispatchFleet {
        fleet_id: 42,
        destination_id: 7,
    });
    let status = result.unwrap();
    assert_eq!(status.fleet_id, 42);
    assert!(status.ok);
}

#[test]
fn handler_axum_json_shim_round_trips() {
    let mut world = galeon_engine::World::new();
    let v = dispatch_fleet__galeon_axum_json(r#"{"fleet_id":3,"destination_id":9}"#, &mut world)
        .unwrap();
    assert_eq!(
        v,
        galeon_engine::serde_json::json!({"fleet_id":3,"ok":true})
    );
}

#[test]
fn handler_with_system_param_axum_json_shim_compiles() {
    let mut world = galeon_engine::World::new();
    world.insert_resource(FleetConfig { max_fleet_size: 10 });
    let v = handler_with_system_param__galeon_axum_json(
        r#"{"fleet_id":1,"destination_id":2}"#,
        &mut world,
    )
    .unwrap();
    assert_eq!(
        v,
        galeon_engine::serde_json::json!({"fleet_id":1,"ok":true})
    );
}

#[test]
fn validate_handlers_passes_for_protocol_request_types() {
    // All handlers in this test file use DispatchFleet (command) or
    // GetFleetStatus (query) as request types — both are registered
    // protocol items, so validation should pass.
    let result = galeon_engine::manifest::ProtocolManifest::validate_handlers();
    assert!(
        result.is_ok(),
        "validate_handlers failed: {:?}",
        result.unwrap_err()
    );
}
