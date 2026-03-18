// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

use proc_macro::TokenStream;
use quote::quote;

/// Derive macro that implements the `Component` trait for a struct.
///
/// This enables the type to be stored in ECS sparse-set component storage,
/// keyed by `TypeId`.
#[proc_macro_derive(Component)]
pub fn derive_component(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let expanded = quote! {
        impl #impl_generics galeon_engine::component::Component for #name #ty_generics #where_clause {}
    };

    TokenStream::from(expanded)
}
