// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

//! #[handler] on a function that does not return Result should fail.

#[galeon_engine::handler]
pub fn bad_handler(cmd: String) -> String {
    cmd
}

fn main() {}
