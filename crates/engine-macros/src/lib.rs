// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

use proc_macro::TokenStream;

/// Derive macro for ECS components (no-op placeholder).
#[proc_macro_derive(Component)]
pub fn derive_component(input: TokenStream) -> TokenStream {
    let _input = syn::parse_macro_input!(input as syn::DeriveInput);
    TokenStream::new()
}
