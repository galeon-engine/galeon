// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

use proc_macro::TokenStream;
use proc_macro_crate::{FoundCrate, crate_name};
use quote::quote;
use syn::spanned::Spanned;
use syn::{Token, parse::Parser, punctuated::Punctuated};

/// Resolve the `galeon-engine` crate path as used by the consumer.
/// Handles renames like `engine = { package = "galeon-engine" }`.
fn engine_crate() -> proc_macro2::TokenStream {
    match crate_name("galeon-engine").expect("galeon-engine must be in Cargo.toml") {
        FoundCrate::Itself => quote!(crate),
        FoundCrate::Name(name) => {
            let ident = syn::Ident::new(&name, proc_macro2::Span::call_site());
            quote!(#ident)
        }
    }
}

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

/// Extract the first `#[doc = "..."]` attribute value as a doc string.
fn extract_doc(attrs: &[syn::Attribute]) -> String {
    for attr in attrs {
        if attr.path().is_ident("doc")
            && let syn::Meta::NameValue(nv) = &attr.meta
            && let syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(s),
                ..
            }) = &nv.value
        {
            return s.value().trim().to_string();
        }
    }
    String::new()
}

/// Convert a type to its string representation for manifest field metadata.
///
/// Collapses runs of whitespace to single spaces but preserves them so
/// composite types like `Vec < ShipView >` render as `Vec<ShipView>`.
fn type_to_string(ty: &syn::Type) -> String {
    let raw = quote!(#ty).to_string();
    raw.replace(" < ", "<")
        .replace("< ", "<")
        .replace(" >", ">")
        .replace(" ,", ",")
        .replace(" ::", "::")
        .replace(":: ", "::")
}

/// Parse optional `surface = "..."` or `surfaces = ["...", "..."]` arguments.
fn parse_surfaces(attr: TokenStream) -> Result<Vec<String>, syn::Error> {
    if attr.is_empty() {
        return Ok(Vec::new());
    }

    let parser = Punctuated::<syn::MetaNameValue, Token![,]>::parse_terminated;
    let args = parser.parse(attr)?;
    let mut surfaces = Vec::new();
    let mut saw_surface = false;
    let mut saw_surfaces = false;

    for arg in args {
        if arg.path.is_ident("surface") {
            if saw_surfaces {
                return Err(syn::Error::new(
                    arg.path.span(),
                    "use either `surface = \"...\"` or `surfaces = [..]`, not both",
                ));
            }
            if saw_surface {
                return Err(syn::Error::new(
                    arg.path.span(),
                    "`surface` may only be specified once",
                ));
            }
            let syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(surface),
                ..
            }) = arg.value
            else {
                return Err(syn::Error::new(
                    arg.path.span(),
                    "`surface` must be a string literal",
                ));
            };
            let val = surface.value();
            if val.is_empty() {
                return Err(syn::Error::new(
                    surface.span(),
                    "surface name must not be empty",
                ));
            }
            saw_surface = true;
            surfaces.push(val);
            continue;
        }

        if arg.path.is_ident("surfaces") {
            if saw_surface {
                return Err(syn::Error::new(
                    arg.path.span(),
                    "use either `surface = \"...\"` or `surfaces = [..]`, not both",
                ));
            }
            if saw_surfaces {
                return Err(syn::Error::new(
                    arg.path.span(),
                    "`surfaces` may only be specified once",
                ));
            }
            let syn::Expr::Array(array) = arg.value else {
                return Err(syn::Error::new(
                    arg.path.span(),
                    "`surfaces` must be an array of string literals",
                ));
            };
            for elem in array.elems {
                let syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(surface),
                    ..
                }) = elem
                else {
                    return Err(syn::Error::new(
                        arg.path.span(),
                        "`surfaces` must be an array of string literals",
                    ));
                };
                let surface_val = surface.value();
                if surface_val.is_empty() {
                    return Err(syn::Error::new(
                        surface.span(),
                        "surface name must not be empty",
                    ));
                }
                surfaces.push(surface_val);
            }
            saw_surfaces = true;
            continue;
        }

        return Err(syn::Error::new(
            arg.path.span(),
            "unsupported protocol attribute argument; expected `surface` or `surfaces`",
        ));
    }

    surfaces.sort();
    surfaces.dedup();
    Ok(surfaces)
}

/// Shared implementation for protocol attribute macros.
fn protocol_attr(
    attr: TokenStream,
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

    // Resolve the engine crate path, handling renames.
    let krate = engine_crate();

    let marker_trait_ident = syn::Ident::new(marker_trait, proc_macro2::Span::call_site());
    let kind_variant_ident = syn::Ident::new(kind_variant, proc_macro2::Span::call_site());
    let surfaces = match parse_surfaces(attr) {
        Ok(surfaces) => surfaces,
        Err(e) => return e.to_compile_error().into(),
    };
    let surface_literals: Vec<syn::LitStr> = surfaces
        .iter()
        .map(|surface| syn::LitStr::new(surface, proc_macro2::Span::call_site()))
        .collect();

    let extra: Vec<syn::Path> = extra_derives
        .iter()
        .map(|d| syn::parse_str(d).expect("valid derive path"))
        .collect();

    // Build serde crate path string for #[serde(crate = "...")] attribute.
    let serde_crate_path = format!("{}::serde", krate);

    // Extract field metadata for manifest generation.
    let doc_str = extract_doc(&s.attrs);
    let field_entries: Vec<proc_macro2::TokenStream> = match &s.fields {
        syn::Fields::Named(fields) => fields
            .named
            .iter()
            .map(|f| {
                let fname = f.ident.as_ref().unwrap().to_string();
                let ftype = type_to_string(&f.ty);
                quote! {
                    #krate::manifest::FieldEntry {
                        name: #fname,
                        ty: #ftype,
                    }
                }
            })
            .collect(),
        _ => Vec::new(), // Unit struct — no fields.
    };

    let expanded = quote! {
        #[derive(#krate::serde::Serialize, #krate::serde::Deserialize, #(#extra),*)]
        #[serde(crate = #serde_crate_path)]
        #item

        impl #impl_generics #krate::protocol::#marker_trait_ident for #name #ty_generics #where_clause {}

        impl #impl_generics #krate::protocol::ProtocolMeta for #name #ty_generics #where_clause {
            fn name() -> &'static str {
                #name_str
            }
            fn kind() -> #krate::protocol::ProtocolKind {
                #krate::protocol::ProtocolKind::#kind_variant_ident
            }
        }

        #krate::inventory::submit! {
            #krate::manifest::ProtocolRegistration {
                name: #name_str,
                kind: #krate::protocol::ProtocolKind::#kind_variant_ident,
                fields: &[#(#field_entries),*],
                doc: #doc_str,
                surfaces: &[#(#surface_literals),*],
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
pub fn command(attr: TokenStream, input: TokenStream) -> TokenStream {
    protocol_attr(attr, input, "Command", "Command", &[])
}

/// Marks a struct as a protocol **query** (read-only request).
///
/// Derives `Serialize`, `Deserialize`, implements [`ProtocolQuery`] and [`ProtocolMeta`].
///
/// # Example
///
/// ```ignore
/// #[galeon_engine::query]
/// pub struct GetFleetSnapshot;
/// ```
#[proc_macro_attribute]
pub fn query(attr: TokenStream, input: TokenStream) -> TokenStream {
    protocol_attr(attr, input, "ProtocolQuery", "Query", &[])
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
pub fn event(attr: TokenStream, input: TokenStream) -> TokenStream {
    protocol_attr(attr, input, "Event", "Event", &[])
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
pub fn dto(attr: TokenStream, input: TokenStream) -> TokenStream {
    protocol_attr(attr, input, "Dto", "Dto", &["Clone"])
}
