// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

//! Negative test for `ProtocolManifest::validate_handlers()` (#162).
//!
//! Proves that a handler whose request type is not a registered protocol
//! item is caught by the validation step. This is a separate binary
//! because `inventory` collects per-binary — mixing valid and invalid
//! handlers in the same test would break the positive-path assertion.

use galeon_engine::manifest::ProtocolManifest;

// A protocol item so the manifest is not empty.
#[galeon_engine::command]
pub struct LegitCommand {
    pub id: u64,
}

// A handler with a non-protocol request type. The IntoHandler assertion
// passes (String satisfies the function signature), but validate_handlers()
// must reject it because String is not a registered protocol item.
#[galeon_engine::handler]
pub fn bad_handler(cmd: String) -> Result<String, String> {
    Ok(cmd)
}

#[test]
fn validate_handlers_rejects_non_protocol_request_type() {
    let result = ProtocolManifest::validate_handlers();
    assert!(
        result.is_err(),
        "expected Err for non-protocol request type"
    );
    let errors = result.unwrap_err();
    assert_eq!(errors.len(), 1);
    assert!(
        errors[0].contains("bad_handler"),
        "error should name the handler: {}",
        errors[0],
    );
    assert!(
        errors[0].contains("String"),
        "error should name the request type: {}",
        errors[0],
    );
}
