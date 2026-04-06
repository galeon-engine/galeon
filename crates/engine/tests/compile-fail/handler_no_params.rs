// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

//! #[handler] on a function with no parameters should fail.

#[galeon_engine::handler]
pub fn bad_handler() -> Result<String, String> {
    Ok("no request".into())
}

fn main() {}
