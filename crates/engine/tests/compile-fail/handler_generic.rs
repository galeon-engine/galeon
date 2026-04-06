// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

//! #[handler] on a generic fn should fail — metadata would contain
//! placeholder type names instead of concrete types.

#[galeon_engine::handler]
pub fn bad_handler<T>(cmd: T) -> Result<T, String> {
    Ok(cmd)
}

fn main() {}
