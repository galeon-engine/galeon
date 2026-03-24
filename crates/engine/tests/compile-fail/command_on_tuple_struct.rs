// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

//! Applying #[galeon_engine::command] to a tuple struct should fail.

#[galeon_engine::command]
pub struct BadCommand(u64, u64);

fn main() {}
