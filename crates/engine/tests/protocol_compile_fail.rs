// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

//! Compile-fail tests for protocol attribute macros (#46/T8).

#[test]
fn compile_fail() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/compile-fail/*.rs");
}
