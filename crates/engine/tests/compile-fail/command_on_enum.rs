// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

//! Applying #[galeon_engine::command] to an enum should fail.

#[galeon_engine::command]
pub enum BadCommand {
    A,
    B,
}

fn main() {}
