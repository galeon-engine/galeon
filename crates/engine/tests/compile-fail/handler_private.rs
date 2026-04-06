// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

//! #[handler] on a private fn should fail.

#[galeon_engine::handler]
fn bad_handler(cmd: String) -> Result<String, String> {
    Ok(cmd)
}

fn main() {}
