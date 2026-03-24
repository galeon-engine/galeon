// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

use proc_macro::TokenStream;
use quote::quote;
use syn::spanned::Spanned;

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

// ---------------------------------------------------------------------------
// Protocol attribute macros
// ---------------------------------------------------------------------------

/// Validate that the input is a named or unit struct (not enum, union, or tuple struct).
fn validate_struct(item: &syn::Item) -> Result<&syn::ItemStruct, syn::Error> {
    match item {
        syn::Item::Struct(s) => {
            if let syn::Fields::Unnamed(_) = &s.fields {
                Err(syn::Error::new(
                    s.fields.span(),
                    "galeon protocol macros do not support tuple structs",
                ))
            } else {
                Ok(s)
            }
        }
        syn::Item::Enum(e) => Err(syn::Error::new(
            e.enum_token.span(),
            "galeon protocol macros do not support enums",
        )),
        _ => Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "galeon protocol macros can only be applied to structs",
        )),
    }
}

/// Shared implementation for protocol attribute macros.
fn protocol_attr(
    input: TokenStream,
    marker_trait: &str,
    kind_variant: &str,
    extra_derives: &[&str],
) -> TokenStream {
    let item: syn::Item = match syn::parse(input) {
        Ok(item) => item,
        Err(e) => return e.to_compile_error().into(),
    };

    let s = match validate_struct(&item) {
        Ok(s) => s,
        Err(e) => return e.to_compile_error().into(),
    };

    let name = &s.ident;
    let name_str = name.to_string();
    let (impl_generics, ty_generics, where_clause) = s.generics.split_for_impl();

    let marker_path: syn::Path =
        syn::parse_str(&format!("galeon_engine::protocol::{marker_trait}"))
            .expect("valid marker trait path");
    let kind_path: syn::Path = syn::parse_str(&format!(
        "galeon_engine::protocol::ProtocolKind::{kind_variant}"
    ))
    .expect("valid kind variant path");

    let extra: Vec<syn::Path> = extra_derives
        .iter()
        .map(|d| syn::parse_str(d).expect("valid derive path"))
        .collect();

    let expanded = quote! {
        #[derive(::serde::Serialize, ::serde::Deserialize, #(#extra),*)]
        #item

        impl #impl_generics #marker_path for #name #ty_generics #where_clause {}

        impl #impl_generics galeon_engine::protocol::ProtocolMeta for #name #ty_generics #where_clause {
            fn name() -> &'static str {
                #name_str
            }
            fn kind() -> galeon_engine::protocol::ProtocolKind {
                #kind_path
            }
        }
    };

    expanded.into()
}

/// Marks a struct as a protocol **command** (state-changing request).
///
/// Derives `Serialize`, `Deserialize`, implements [`Command`] and [`ProtocolMeta`].
///
/// # Example
///
/// ```ignore
/// #[galeon_engine::command]
/// pub struct DispatchShip {
///     pub ship_id: u64,
///     pub contract_id: u64,
/// }
/// ```
#[proc_macro_attribute]
pub fn command(_attr: TokenStream, input: TokenStream) -> TokenStream {
    protocol_attr(input, "Command", "Command", &[])
}

/// Marks a struct as a protocol **query** (read-only request).
///
/// Derives `Serialize`, `Deserialize`, implements [`Query`] and [`ProtocolMeta`].
///
/// # Example
///
/// ```ignore
/// #[galeon_engine::query]
/// pub struct GetFleetSnapshot;
/// ```
#[proc_macro_attribute]
pub fn query(_attr: TokenStream, input: TokenStream) -> TokenStream {
    protocol_attr(input, "Query", "Query", &[])
}

/// Marks a struct as a protocol **event** (authoritative fact).
///
/// Derives `Serialize`, `Deserialize`, implements [`Event`] and [`ProtocolMeta`].
///
/// # Example
///
/// ```ignore
/// #[galeon_engine::event]
/// pub struct ShipArrived {
///     pub ship_id: u64,
///     pub arrived_at: u64,
/// }
/// ```
#[proc_macro_attribute]
pub fn event(_attr: TokenStream, input: TokenStream) -> TokenStream {
    protocol_attr(input, "Event", "Event", &[])
}

/// Marks a struct as a protocol **DTO** (boundary-facing data structure).
///
/// Derives `Serialize`, `Deserialize`, `Clone`, implements [`Dto`] and [`ProtocolMeta`].
///
/// # Example
///
/// ```ignore
/// #[galeon_engine::dto]
/// pub struct FleetSnapshot {
///     pub ships: Vec<ShipView>,
/// }
/// ```
#[proc_macro_attribute]
pub fn dto(_attr: TokenStream, input: TokenStream) -> TokenStream {
    protocol_attr(input, "Dto", "Dto", &["Clone"])
}
