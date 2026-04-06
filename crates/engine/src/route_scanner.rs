// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

//! Filesystem-routed API scanning and axum glue generation.
//!
//! Walks an `api/` directory to discover route files, matches them against
//! [`HandlerRegistration`] entries collected via `inventory`, and emits
//! `generated/routes.rs` — an axum `Router` that delegates through the
//! [`HandlerRegistry`] JSON dispatch boundary.
//!
//! # Data flow
//!
//! ```text
//! CLI walks api/         HandlerRegistration    ProtocolManifest
//!       │                  (inventory)              (inventory)
//!       ▼                       │                       │
//!  scan_api_routes()            │                       │
//!       │                       ▼                       ▼
//!       └──────────►  resolve_routes()  ◄───────────────┘
//!                           │
//!                           ▼
//!                  generate_axum_routes()
//!                           │
//!                           ▼
//!                   generated/routes.rs
//! ```
//!
//! Schema always comes from [`ProtocolManifest`]; execution always goes
//! through [`HandlerRegistry`]. The scanner discovers *exposure* — which
//! handlers are reachable via HTTP — without re-deriving protocol schema.

use crate::manifest::{HandlerRegistration, ProtocolManifest};
use crate::protocol::ProtocolKind;
use serde::{Deserialize, Serialize};
use std::path::Path;

// =============================================================================
// T1: Filesystem scanning and path normalization
// =============================================================================

/// A discovered route file from the `api/` directory.
///
/// Produced by [`scan_api_routes`] from a list of relative file paths.
/// Each entry maps a source file to its HTTP route path and the Rust module
/// path suffix used to match against [`HandlerRegistration::module_path`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScannedRoute {
    /// HTTP route path (e.g., `"/api/fleet/dispatch"`).
    pub route_path: String,
    /// Module path suffix for handler matching (e.g., `"api::fleet::dispatch"`).
    pub module_suffix: String,
}

/// Scan a list of relative file paths and produce normalized route entries.
///
/// `relative_paths` must be relative to the project root and use forward
/// slashes (e.g., `"api/fleet/dispatch.rs"`). The function:
///
/// - Keeps only `.rs` files
/// - Skips files whose stem starts with `_` (helper modules, not routes)
/// - Skips `mod.rs` (module root, not an endpoint)
/// - Normalizes `\` to `/` for cross-platform compatibility
/// - Sorts results by route path for deterministic output
pub fn scan_api_routes(relative_paths: &[&str]) -> Vec<ScannedRoute> {
    let mut routes = Vec::new();

    for &raw_path in relative_paths {
        let normalized_input = raw_path.replace('\\', "/");
        let path = Path::new(&normalized_input);

        // Must have .rs extension.
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }

        let file_stem = match path.file_stem().and_then(|s| s.to_str()) {
            Some(stem) => stem,
            None => continue,
        };

        // Skip _-prefixed helper files.
        if file_stem.starts_with('_') {
            continue;
        }

        // Skip mod.rs — module root, not a route endpoint.
        if file_stem == "mod" {
            continue;
        }

        // Strip .rs extension, normalize separators.
        let without_ext = path.with_extension("");
        let clean = without_ext.to_string_lossy().replace('\\', "/");

        let route_path = format!("/{clean}");
        let module_suffix = clean.replace('/', "::");

        routes.push(ScannedRoute {
            route_path,
            module_suffix,
        });
    }

    routes.sort_by(|a, b| a.route_path.cmp(&b.route_path));
    routes
}

// =============================================================================
// T2 + T3: Route resolution — join scanner, handlers, and manifest
// =============================================================================

/// Owned handler metadata extracted from [`HandlerRegistration`].
///
/// `HandlerRegistration` uses `&'static str` fields (baked in by macros).
/// This owned copy is serializable and decoupled from the `inventory` lifetime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandlerMeta {
    /// Handler function name (e.g., `"dispatch_fleet"`).
    pub name: String,
    /// Full module path (e.g., `"my_game::api::fleet::dispatch"`).
    pub module_path: String,
    /// Request type name (e.g., `"DispatchFleetCmd"`).
    pub request_type: String,
    /// Response type name (e.g., `"FleetStatus"`).
    pub response_type: String,
    /// Error type name (e.g., `"FleetError"`).
    pub error_type: String,
}

impl HandlerMeta {
    /// Convert a static [`HandlerRegistration`] into an owned [`HandlerMeta`].
    pub fn from_registration(reg: &HandlerRegistration) -> Self {
        Self {
            name: reg.name.to_string(),
            module_path: reg.module_path.to_string(),
            request_type: reg.request_type.to_string(),
            response_type: reg.response_type.to_string(),
            error_type: reg.error_type.to_string(),
        }
    }

    /// Collect all [`HandlerRegistration`] entries from `inventory` into
    /// owned [`HandlerMeta`] values.
    pub fn collect_all() -> Vec<Self> {
        inventory::iter::<HandlerRegistration>
            .into_iter()
            .map(Self::from_registration)
            .collect()
    }
}

/// A fully resolved route with all metadata needed for code generation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedRoute {
    /// HTTP route path (e.g., `"/api/fleet/dispatch"`).
    pub route_path: String,
    /// Handler function name (e.g., `"dispatch_fleet"`).
    pub handler_fn_name: String,
    /// Protocol name of the request type (e.g., `"DispatchFleetCmd"`).
    pub protocol_name: String,
    /// Protocol kind — determines dispatch method.
    pub kind: ProtocolKind,
    /// Explicit surface memberships from the manifest entry.
    /// Empty means "default surface only" — same semantics as [`ManifestEntry::surfaces`].
    pub surfaces: Vec<String>,
}

/// Match scanned routes to handler registrations and manifest entries.
///
/// For each [`ScannedRoute`], finds the matching [`HandlerMeta`] by checking
/// whether `handler.module_path` ends with the route's `module_suffix`.
/// Then looks up the handler's `request_type` in the manifest to determine
/// the protocol kind and surface membership.
///
/// Returns resolved routes on success, or a list of diagnostic messages
/// for unmatched routes or ambiguous matches.
pub fn resolve_routes(
    scanned: &[ScannedRoute],
    handlers: &[HandlerMeta],
    manifest: &ProtocolManifest,
) -> Result<Vec<ResolvedRoute>, Vec<String>> {
    let mut resolved = Vec::new();
    let mut errors = Vec::new();

    for route in scanned {
        // Find handlers whose module_path ends with the route's module suffix.
        let suffix_with_sep = format!("::{}", route.module_suffix);
        let candidates: Vec<&HandlerMeta> = handlers
            .iter()
            .filter(|h| {
                h.module_path.ends_with(&suffix_with_sep) || h.module_path == route.module_suffix
            })
            .collect();

        if candidates.is_empty() {
            errors.push(format!(
                "route {} has no matching handler (expected module suffix '{}')",
                route.route_path, route.module_suffix,
            ));
            continue;
        }

        if candidates.len() > 1 {
            let names: Vec<&str> = candidates.iter().map(|h| h.name.as_str()).collect();
            errors.push(format!(
                "route {} matches {} handlers: {}",
                route.route_path,
                candidates.len(),
                names.join(", "),
            ));
            continue;
        }

        let handler = candidates[0];

        // Look up the request type in the manifest to determine kind + surfaces.
        let (kind, protocol_name, surfaces) =
            match lookup_protocol_entry(manifest, &handler.request_type) {
                Some(result) => result,
                None => {
                    errors.push(format!(
                        "route {} handler '{}' has request type '{}' not found in manifest",
                        route.route_path, handler.name, handler.request_type,
                    ));
                    continue;
                }
            };

        resolved.push(ResolvedRoute {
            route_path: route.route_path.clone(),
            handler_fn_name: handler.name.clone(),
            protocol_name,
            kind,
            surfaces,
        });
    }

    if errors.is_empty() {
        Ok(resolved)
    } else {
        Err(errors)
    }
}

/// Look up a request type name in the manifest.
///
/// Returns the protocol kind, canonical manifest name, and surface memberships.
///
/// The `request_type` from [`HandlerRegistration`] may include a path prefix
/// (e.g., `"crate::SpawnUnit"`) because the `#[handler]` macro preserves the
/// type path as written in source. We match against the last segment of the
/// path to find the corresponding manifest entry.
fn lookup_protocol_entry(
    manifest: &ProtocolManifest,
    request_type: &str,
) -> Option<(ProtocolKind, String, Vec<String>)> {
    let type_name = request_type.rsplit("::").next().unwrap_or(request_type);

    for entry in &manifest.commands {
        if entry.name == type_name {
            return Some((
                ProtocolKind::Command,
                entry.name.clone(),
                entry.surfaces.clone(),
            ));
        }
    }
    for entry in &manifest.queries {
        if entry.name == type_name {
            return Some((
                ProtocolKind::Query,
                entry.name.clone(),
                entry.surfaces.clone(),
            ));
        }
    }
    None
}

/// Strip any module path prefix from a request type name.
///
/// `"crate::SpawnUnit"` → `"SpawnUnit"`, `"SpawnUnit"` → `"SpawnUnit"`.
///
/// Shared between route resolution and handler validation so both
/// entrypoints behave consistently.
pub fn strip_type_prefix(qualified: &str) -> &str {
    qualified.rsplit("::").next().unwrap_or(qualified)
}

// =============================================================================
// T4: Axum glue code generation
// =============================================================================

/// Generate the `routes.rs` axum glue source code from resolved routes.
///
/// The generated code creates per-surface axum `Router` functions with
/// `Arc<HandlerRegistry>` as state. Each route uses POST and delegates to
/// [`HandlerRegistry::dispatch_command_json`] or
/// [`HandlerRegistry::dispatch_query_json`] using the protocol name
/// from the manifest.
///
/// All routes are POST — the manifest cannot distinguish unit structs
/// (`null`) from empty named structs (`{}`), so GET with a hardcoded
/// payload would break one or the other at runtime.
///
/// For single-surface manifests the function is `pub fn router()`.
/// For multi-surface manifests each surface gets its own function
/// (e.g., `pub fn gameplay_router()`).
pub fn generate_axum_routes(routes: &[ResolvedRoute], manifest: &ProtocolManifest) -> String {
    let mut out = String::new();

    out.push_str("// Auto-generated by Galeon Engine — do not edit.\n");
    out.push_str(&format!("// Protocol: {}\n\n", manifest.protocol_version));

    out.push_str("use axum::extract::State;\n");
    out.push_str("use axum::http::StatusCode;\n");
    out.push_str("use axum::{Router, routing};\n");
    out.push_str("use galeon_engine::HandlerRegistry;\n");
    out.push_str("use std::sync::Arc;\n");
    out.push('\n');

    // Per-surface router functions.
    let surface_names = manifest.resolved_surface_names();

    if surface_names.len() == 1 {
        // Single surface — generate a plain `router()`.
        emit_router_fn(&mut out, "router", routes, |_| true);
    } else {
        // Multi-surface — generate `<surface>_router()` per surface.
        let default_surface = &manifest.default_surface;
        for surface in &surface_names {
            let fn_name = format!("{}_router", surface.replace('-', "_"));
            emit_router_fn(&mut out, &fn_name, routes, |r| {
                route_belongs_to_surface(r, surface, default_surface)
            });
        }
    }

    // Handler functions — shared across surfaces, deduplicated.
    let mut emitted: std::collections::HashSet<String> = std::collections::HashSet::new();
    for route in routes {
        let handler_name = route_to_handler_ident(&route.route_path);
        if !emitted.insert(handler_name.clone()) {
            continue;
        }
        let dispatch_method = match route.kind {
            ProtocolKind::Command => "dispatch_command_json",
            ProtocolKind::Query => "dispatch_query_json",
            _ => "dispatch_command_json",
        };

        out.push_str(&format!(
            "async fn {handler_name}(\n\
             \x20   State(registry): State<Arc<HandlerRegistry>>,\n\
             \x20   body: String,\n\
             ) -> Result<String, (StatusCode, String)> {{\n\
             \x20   registry\n\
             \x20       .{dispatch_method}({}, &body)\n\
             \x20       .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))\n\
             }}\n\n",
            quote_str(&route.protocol_name),
        ));
    }

    out
}

/// Emit a single router function containing the routes that pass `filter`.
fn emit_router_fn(
    out: &mut String,
    fn_name: &str,
    routes: &[ResolvedRoute],
    filter: impl Fn(&ResolvedRoute) -> bool,
) {
    out.push_str(&format!(
        "pub fn {fn_name}() -> Router<Arc<HandlerRegistry>> {{\n"
    ));
    out.push_str("    Router::new()\n");
    for route in routes.iter().filter(|r| filter(r)) {
        let handler_name = route_to_handler_ident(&route.route_path);
        out.push_str(&format!(
            "        .route({}, routing::post({handler_name}))\n",
            quote_str(&route.route_path),
        ));
    }
    out.push_str("}\n\n");
}

/// Check whether a resolved route belongs to a surface, using the same
/// semantics as [`ProtocolManifest::entry_belongs_to_surface`].
fn route_belongs_to_surface(route: &ResolvedRoute, surface: &str, default_surface: &str) -> bool {
    if route.surfaces.is_empty() {
        surface == default_surface
    } else {
        route.surfaces.iter().any(|s| s == surface)
    }
}

/// Convert a route path to a valid Rust function identifier.
///
/// `/api/fleet/dispatch` → `api_fleet_dispatch`
fn route_to_handler_ident(route_path: &str) -> String {
    route_path.trim_start_matches('/').replace(['/', '-'], "_")
}

/// Quote a string as a Rust string literal.
fn quote_str(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{ManifestEntry, ManifestField};

    // -- T1: scan_api_routes --

    #[test]
    fn scan_basic_route_files() {
        let paths = &[
            "api/fleet/dispatch.rs",
            "api/fleet/snapshot.rs",
            "api/physics/apply_force.rs",
        ];
        let routes = scan_api_routes(paths);
        assert_eq!(routes.len(), 3);

        assert_eq!(routes[0].route_path, "/api/fleet/dispatch");
        assert_eq!(routes[0].module_suffix, "api::fleet::dispatch");

        assert_eq!(routes[1].route_path, "/api/fleet/snapshot");
        assert_eq!(routes[1].module_suffix, "api::fleet::snapshot");

        assert_eq!(routes[2].route_path, "/api/physics/apply_force");
        assert_eq!(routes[2].module_suffix, "api::physics::apply_force");
    }

    #[test]
    fn scan_skips_underscore_prefixed_files() {
        let paths = &[
            "api/fleet/dispatch.rs",
            "api/_types.rs",
            "api/fleet/_helpers.rs",
        ];
        let routes = scan_api_routes(paths);
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].route_path, "/api/fleet/dispatch");
    }

    #[test]
    fn scan_skips_mod_rs() {
        let paths = &["api/fleet/mod.rs", "api/fleet/dispatch.rs"];
        let routes = scan_api_routes(paths);
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].route_path, "/api/fleet/dispatch");
    }

    #[test]
    fn scan_skips_non_rs_files() {
        let paths = &[
            "api/fleet/dispatch.rs",
            "api/fleet/README.md",
            "api/fleet/data.json",
        ];
        let routes = scan_api_routes(paths);
        assert_eq!(routes.len(), 1);
    }

    #[test]
    fn scan_normalizes_backslashes() {
        let paths = &["api\\fleet\\dispatch.rs"];
        let routes = scan_api_routes(paths);
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].route_path, "/api/fleet/dispatch");
        assert_eq!(routes[0].module_suffix, "api::fleet::dispatch");
    }

    #[test]
    fn scan_deterministic_sort_order() {
        let paths = &["api/zzz/last.rs", "api/aaa/first.rs", "api/mmm/middle.rs"];
        let routes = scan_api_routes(paths);
        assert_eq!(routes[0].route_path, "/api/aaa/first");
        assert_eq!(routes[1].route_path, "/api/mmm/middle");
        assert_eq!(routes[2].route_path, "/api/zzz/last");
    }

    #[test]
    fn scan_empty_input() {
        let routes = scan_api_routes(&[]);
        assert!(routes.is_empty());
    }

    // -- T2 + T3: resolve_routes --

    fn sample_handlers() -> Vec<HandlerMeta> {
        vec![
            HandlerMeta {
                name: "dispatch_fleet".into(),
                module_path: "my_game::api::fleet::dispatch".into(),
                request_type: "DispatchFleetCmd".into(),
                response_type: "FleetStatus".into(),
                error_type: "String".into(),
            },
            HandlerMeta {
                name: "fleet_snapshot".into(),
                module_path: "my_game::api::fleet::snapshot".into(),
                request_type: "GetFleetSnapshot".into(),
                response_type: "FleetSnapshot".into(),
                error_type: "String".into(),
            },
        ]
    }

    fn sample_manifest_for_routes() -> ProtocolManifest {
        ProtocolManifest {
            manifest_version: "2".into(),
            protocol_version: "test@0.1".into(),
            default_surface: "default".into(),
            surfaces: vec!["default".into()],
            commands: vec![ManifestEntry {
                name: "DispatchFleetCmd".into(),
                kind: ProtocolKind::Command,
                fields: vec![ManifestField {
                    name: "fleet_id".into(),
                    ty: "u64".into(),
                }],
                doc: "".into(),
                surfaces: vec![],
            }],
            queries: vec![ManifestEntry {
                name: "GetFleetSnapshot".into(),
                kind: ProtocolKind::Query,
                fields: vec![],
                doc: "".into(),
                surfaces: vec![],
            }],
            events: vec![],
            dtos: vec![],
        }
    }

    #[test]
    fn resolve_matches_handlers_by_module_suffix() {
        let scanned = scan_api_routes(&["api/fleet/dispatch.rs", "api/fleet/snapshot.rs"]);
        let handlers = sample_handlers();
        let manifest = sample_manifest_for_routes();

        let resolved = resolve_routes(&scanned, &handlers, &manifest).unwrap();
        assert_eq!(resolved.len(), 2);

        assert_eq!(resolved[0].route_path, "/api/fleet/dispatch");
        assert_eq!(resolved[0].handler_fn_name, "dispatch_fleet");
        assert_eq!(resolved[0].protocol_name, "DispatchFleetCmd");
        assert_eq!(resolved[0].kind, ProtocolKind::Command);
        assert!(resolved[0].surfaces.is_empty()); // inherits default

        assert_eq!(resolved[1].route_path, "/api/fleet/snapshot");
        assert_eq!(resolved[1].handler_fn_name, "fleet_snapshot");
        assert_eq!(resolved[1].protocol_name, "GetFleetSnapshot");
        assert_eq!(resolved[1].kind, ProtocolKind::Query);
        assert!(resolved[1].surfaces.is_empty());
    }

    #[test]
    fn resolve_errors_on_unmatched_route() {
        let scanned = scan_api_routes(&["api/unknown/route.rs"]);
        let handlers = sample_handlers();
        let manifest = sample_manifest_for_routes();

        let err = resolve_routes(&scanned, &handlers, &manifest).unwrap_err();
        assert_eq!(err.len(), 1);
        assert!(err[0].contains("no matching handler"));
        assert!(err[0].contains("api::unknown::route"));
    }

    #[test]
    fn resolve_errors_on_missing_manifest_entry() {
        let scanned = scan_api_routes(&["api/fleet/dispatch.rs"]);
        let handlers = vec![HandlerMeta {
            name: "dispatch_fleet".into(),
            module_path: "my_game::api::fleet::dispatch".into(),
            request_type: "UnknownType".into(),
            response_type: "()".into(),
            error_type: "String".into(),
        }];
        let manifest = sample_manifest_for_routes();

        let err = resolve_routes(&scanned, &handlers, &manifest).unwrap_err();
        assert_eq!(err.len(), 1);
        assert!(err[0].contains("not found in manifest"));
    }

    #[test]
    fn resolve_carries_surface_membership() {
        let scanned = scan_api_routes(&["api/admin/reset.rs"]);
        let handlers = vec![HandlerMeta {
            name: "admin_reset".into(),
            module_path: "my_game::api::admin::reset".into(),
            request_type: "AdminReset".into(),
            response_type: "()".into(),
            error_type: "String".into(),
        }];
        let manifest = ProtocolManifest {
            manifest_version: "2".into(),
            protocol_version: "test@0.1".into(),
            default_surface: "gameplay".into(),
            surfaces: vec!["authority".into(), "gameplay".into()],
            commands: vec![ManifestEntry {
                name: "AdminReset".into(),
                kind: ProtocolKind::Command,
                fields: vec![],
                doc: "".into(),
                surfaces: vec!["authority".into()],
            }],
            queries: vec![],
            events: vec![],
            dtos: vec![],
        };

        let resolved = resolve_routes(&scanned, &handlers, &manifest).unwrap();
        assert_eq!(resolved[0].surfaces, vec!["authority".to_string()]);
    }

    // -- T4: generate_axum_routes --

    fn sample_manifest_single_surface() -> ProtocolManifest {
        sample_manifest_for_routes()
    }

    #[test]
    fn generate_routes_contains_header() {
        let manifest = sample_manifest_single_surface();
        let code = generate_axum_routes(&[], &manifest);
        assert!(code.contains("Auto-generated by Galeon Engine"));
        assert!(code.contains("test@0.1"));
    }

    #[test]
    fn generate_routes_post_command() {
        let manifest = sample_manifest_single_surface();
        let routes = vec![ResolvedRoute {
            route_path: "/api/fleet/dispatch".into(),
            handler_fn_name: "dispatch_fleet".into(),
            protocol_name: "DispatchFleetCmd".into(),
            kind: ProtocolKind::Command,
            surfaces: vec![],
        }];

        let code = generate_axum_routes(&routes, &manifest);
        assert!(code.contains("routing::post(api_fleet_dispatch)"));
        assert!(code.contains("\"/api/fleet/dispatch\""));
        assert!(code.contains("dispatch_command_json"));
        assert!(code.contains("\"DispatchFleetCmd\""));
        assert!(code.contains("body: String"));
    }

    #[test]
    fn generate_routes_query_is_post() {
        let manifest = sample_manifest_single_surface();
        let routes = vec![ResolvedRoute {
            route_path: "/api/fleet/snapshot".into(),
            handler_fn_name: "fleet_snapshot".into(),
            protocol_name: "GetFleetSnapshot".into(),
            kind: ProtocolKind::Query,
            surfaces: vec![],
        }];

        let code = generate_axum_routes(&routes, &manifest);
        // All routes are POST — avoids unit-struct vs empty-named-struct ambiguity.
        assert!(code.contains("routing::post(api_fleet_snapshot)"));
        assert!(code.contains("dispatch_query_json"));
        assert!(code.contains("body: String"));
        assert!(!code.contains("\"null\""));
    }

    #[test]
    fn generate_routes_multiple_routes() {
        let manifest = sample_manifest_single_surface();
        let routes = vec![
            ResolvedRoute {
                route_path: "/api/fleet/dispatch".into(),
                handler_fn_name: "dispatch_fleet".into(),
                protocol_name: "DispatchFleetCmd".into(),
                kind: ProtocolKind::Command,
                surfaces: vec![],
            },
            ResolvedRoute {
                route_path: "/api/fleet/snapshot".into(),
                handler_fn_name: "fleet_snapshot".into(),
                protocol_name: "GetFleetSnapshot".into(),
                kind: ProtocolKind::Query,
                surfaces: vec![],
            },
        ];

        let code = generate_axum_routes(&routes, &manifest);
        assert!(code.contains("api_fleet_dispatch"));
        assert!(code.contains("api_fleet_snapshot"));
        assert!(code.contains(".route(\"/api/fleet/dispatch\""));
        assert!(code.contains(".route(\"/api/fleet/snapshot\""));
    }

    #[test]
    fn generate_routes_empty() {
        let manifest = sample_manifest_single_surface();
        let code = generate_axum_routes(&[], &manifest);
        assert!(code.contains("Router::new()"));
        assert!(!code.contains(".route("));
    }

    #[test]
    fn generate_routes_single_surface_uses_plain_router() {
        let manifest = sample_manifest_single_surface();
        let routes = vec![ResolvedRoute {
            route_path: "/api/fleet/dispatch".into(),
            handler_fn_name: "dispatch_fleet".into(),
            protocol_name: "DispatchFleetCmd".into(),
            kind: ProtocolKind::Command,
            surfaces: vec![],
        }];

        let code = generate_axum_routes(&routes, &manifest);
        assert!(code.contains("pub fn router()"));
        assert!(!code.contains("_router()"));
    }

    #[test]
    fn generate_routes_multi_surface_filters_by_surface() {
        let manifest = ProtocolManifest {
            manifest_version: "2".into(),
            protocol_version: "test@0.1".into(),
            default_surface: "gameplay".into(),
            surfaces: vec!["authority".into(), "gameplay".into()],
            commands: vec![],
            queries: vec![],
            events: vec![],
            dtos: vec![],
        };

        let routes = vec![
            ResolvedRoute {
                route_path: "/api/fleet/dispatch".into(),
                handler_fn_name: "dispatch_fleet".into(),
                protocol_name: "DispatchFleetCmd".into(),
                kind: ProtocolKind::Command,
                surfaces: vec![], // default surface = gameplay
            },
            ResolvedRoute {
                route_path: "/api/admin/reset".into(),
                handler_fn_name: "admin_reset".into(),
                protocol_name: "AdminReset".into(),
                kind: ProtocolKind::Command,
                surfaces: vec!["authority".into()],
            },
        ];

        let code = generate_axum_routes(&routes, &manifest);

        // Per-surface router functions.
        assert!(code.contains("pub fn authority_router()"));
        assert!(code.contains("pub fn gameplay_router()"));
        assert!(!code.contains("pub fn router()"));

        // Extract each router function body (up to closing `}`).
        let extract_router_body = |fn_name: &str| -> String {
            let start = code.find(&format!("pub fn {fn_name}()")).unwrap();
            let rest = &code[start..];
            // The function ends at the first `}\n\n` (closing brace + blank line).
            let end = rest.find("}\n\n").unwrap() + 1;
            rest[..end].to_string()
        };

        let authority_body = extract_router_body("authority_router");
        let gameplay_body = extract_router_body("gameplay_router");

        // authority_router contains only AdminReset
        assert!(authority_body.contains("api_admin_reset"));
        assert!(!authority_body.contains("api_fleet_dispatch"));

        // gameplay_router contains only DispatchFleetCmd
        assert!(gameplay_body.contains("api_fleet_dispatch"));
        assert!(!gameplay_body.contains("api_admin_reset"));
    }

    #[test]
    fn route_to_handler_ident_converts_paths() {
        assert_eq!(
            route_to_handler_ident("/api/fleet/dispatch"),
            "api_fleet_dispatch"
        );
        assert_eq!(
            route_to_handler_ident("/api/fleet-ops/dispatch"),
            "api_fleet_ops_dispatch"
        );
    }
}
