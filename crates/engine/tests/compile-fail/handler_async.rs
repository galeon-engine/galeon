// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

//! #[handler] on an async fn should fail.

#[galeon_engine::handler]
pub async fn bad_handler(cmd: String) -> Result<String, String> {
    Ok(cmd)
}

fn main() {}
