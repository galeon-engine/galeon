// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

use wasm_bindgen::prelude::*;

/// Returns the engine version string to the JS runtime.
#[wasm_bindgen]
pub fn version() -> String {
    galeon_engine::engine_version().to_string()
}
