// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

use std::env;
use std::fmt::Write;

use serde::Deserialize;

use crate::generate;

/// Top-level envelope returned by the `inspect-routes` reflection helper.
#[derive(Deserialize)]
struct InspectOutput {
    default_surface: String,
    routes: Vec<RouteEntry>,
}

/// Deserialized route entry from the reflection helper's JSON output.
///
/// Mirrors `galeon_engine::ResolvedRoute` without depending on the engine
/// crate — the CLI links the engine only through a temporary helper binary.
#[derive(Deserialize)]
struct RouteEntry {
    route_path: String,
    handler_fn_name: String,
    protocol_name: String,
    kind: String,
    surfaces: Vec<String>,
}

pub fn run() -> Result<(), String> {
    let start_dir = env::current_dir().map_err(|e| format!("failed to read current dir: {e}"))?;
    let (json, protocol_version) = generate::inspect_routes_json(&start_dir)?;
    let output: InspectOutput =
        serde_json::from_str(&json).map_err(|e| format!("failed to parse route data: {e}"))?;
    print!(
        "{}",
        format_route_table(&output.routes, &protocol_version, &output.default_surface)
    );
    Ok(())
}

fn format_route_table(
    entries: &[RouteEntry],
    protocol_version: &str,
    default_surface: &str,
) -> String {
    let mut out = String::new();

    let count = entries.len();
    writeln!(
        out,
        "Protocol: {protocol_version} — {count} route{}",
        if count == 1 { "" } else { "s" },
    )
    .unwrap();
    writeln!(out).unwrap();

    if entries.is_empty() {
        writeln!(out, "No routes found.").unwrap();
        return out;
    }

    // Resolve surface labels: empty → default_surface name, non-empty → join.
    let surface_labels: Vec<String> = entries
        .iter()
        .map(|e| {
            if e.surfaces.is_empty() {
                default_surface.to_string()
            } else {
                e.surfaces.join(", ")
            }
        })
        .collect();

    let method_w = "METHOD".len();
    let path_w = entries
        .iter()
        .map(|e| e.route_path.len())
        .max()
        .unwrap_or(0)
        .max("PATH".len());
    let handler_w = entries
        .iter()
        .map(|e| e.handler_fn_name.len())
        .max()
        .unwrap_or(0)
        .max("HANDLER".len());
    let surface_w = surface_labels
        .iter()
        .map(|s| s.len())
        .max()
        .unwrap_or(0)
        .max("SURFACE".len());

    writeln!(
        out,
        "{:<method_w$}  {:<path_w$}  {:<handler_w$}  {:<surface_w$}  REQUEST",
        "METHOD", "PATH", "HANDLER", "SURFACE",
    )
    .unwrap();

    for (entry, surface) in entries.iter().zip(&surface_labels) {
        let kind_label = entry.kind.to_lowercase();
        writeln!(
            out,
            "{:<method_w$}  {:<path_w$}  {:<handler_w$}  {:<surface_w$}  {} ({})",
            "POST",
            entry.route_path,
            entry.handler_fn_name,
            surface,
            entry.protocol_name,
            kind_label,
        )
        .unwrap();
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_entries() -> Vec<RouteEntry> {
        vec![
            RouteEntry {
                route_path: "/api/fleet/dispatch".into(),
                handler_fn_name: "dispatch_fleet".into(),
                protocol_name: "SpawnUnit".into(),
                kind: "Command".into(),
                surfaces: vec![],
            },
            RouteEntry {
                route_path: "/api/fleet/snapshot".into(),
                handler_fn_name: "fleet_snapshot".into(),
                protocol_name: "GetWorldSnapshot".into(),
                kind: "Query".into(),
                surfaces: vec![],
            },
        ]
    }

    #[test]
    fn table_contains_header_and_route_count() {
        let output = format_route_table(&sample_entries(), "fixture-protocol@0.3.1", "default");
        assert!(output.contains("Protocol: fixture-protocol@0.3.1 — 2 routes"));
    }

    #[test]
    fn table_contains_column_headers() {
        let output = format_route_table(&sample_entries(), "test@0.1", "default");
        assert!(output.contains("METHOD"));
        assert!(output.contains("PATH"));
        assert!(output.contains("HANDLER"));
        assert!(output.contains("SURFACE"));
        assert!(output.contains("REQUEST"));
    }

    #[test]
    fn table_contains_route_rows() {
        let output = format_route_table(&sample_entries(), "test@0.1", "default");
        assert!(output.contains("POST"));
        assert!(output.contains("/api/fleet/dispatch"));
        assert!(output.contains("dispatch_fleet"));
        assert!(output.contains("SpawnUnit (command)"));
        assert!(output.contains("/api/fleet/snapshot"));
        assert!(output.contains("fleet_snapshot"));
        assert!(output.contains("GetWorldSnapshot (query)"));
    }

    #[test]
    fn table_sorted_by_path() {
        let output = format_route_table(&sample_entries(), "test@0.1", "default");
        let dispatch_pos = output.find("/api/fleet/dispatch").unwrap();
        let snapshot_pos = output.find("/api/fleet/snapshot").unwrap();
        assert!(dispatch_pos < snapshot_pos);
    }

    #[test]
    fn empty_table_shows_no_routes() {
        let output = format_route_table(&[], "test@0.1", "default");
        assert!(output.contains("0 routes"));
        assert!(output.contains("No routes found."));
    }

    #[test]
    fn singular_route_count() {
        let entries = vec![RouteEntry {
            route_path: "/api/ping".into(),
            handler_fn_name: "ping".into(),
            protocol_name: "Ping".into(),
            kind: "Query".into(),
            surfaces: vec![],
        }];
        let output = format_route_table(&entries, "test@0.1", "default");
        assert!(output.contains("1 route\n"));
    }

    #[test]
    fn empty_surfaces_resolve_to_default_surface_name() {
        let entries = vec![RouteEntry {
            route_path: "/api/fleet/dispatch".into(),
            handler_fn_name: "dispatch_fleet".into(),
            protocol_name: "SpawnUnit".into(),
            kind: "Command".into(),
            surfaces: vec![],
        }];
        let output = format_route_table(&entries, "test@0.1", "gameplay");
        assert!(output.contains("gameplay"));
    }

    #[test]
    fn explicit_surfaces_shown_as_provided() {
        let entries = vec![RouteEntry {
            route_path: "/api/admin/reset".into(),
            handler_fn_name: "admin_reset".into(),
            protocol_name: "AdminReset".into(),
            kind: "Command".into(),
            surfaces: vec!["authority".into()],
        }];
        let output = format_route_table(&entries, "test@0.1", "gameplay");
        assert!(output.contains("authority"));
        assert!(!output.contains("gameplay"));
    }

    #[test]
    fn multi_surface_table_shows_distinct_surfaces() {
        let entries = vec![
            RouteEntry {
                route_path: "/api/admin/reset".into(),
                handler_fn_name: "admin_reset".into(),
                protocol_name: "AdminReset".into(),
                kind: "Command".into(),
                surfaces: vec!["authority".into()],
            },
            RouteEntry {
                route_path: "/api/fleet/dispatch".into(),
                handler_fn_name: "dispatch_fleet".into(),
                protocol_name: "SpawnUnit".into(),
                kind: "Command".into(),
                surfaces: vec![], // → default "gameplay"
            },
            RouteEntry {
                route_path: "/api/shared/status".into(),
                handler_fn_name: "shared_status".into(),
                protocol_name: "GetStatus".into(),
                kind: "Query".into(),
                surfaces: vec!["authority".into(), "gameplay".into()],
            },
        ];
        let output = format_route_table(&entries, "test@0.1", "gameplay");

        // authority-only route
        assert!(output.contains("admin_reset"));
        let admin_line = output.lines().find(|l| l.contains("admin_reset")).unwrap();
        assert!(admin_line.contains("authority"));

        // default-surface route
        let dispatch_line = output
            .lines()
            .find(|l| l.contains("dispatch_fleet"))
            .unwrap();
        assert!(dispatch_line.contains("gameplay"));

        // multi-surface route shows comma-joined
        let status_line = output
            .lines()
            .find(|l| l.contains("shared_status"))
            .unwrap();
        assert!(status_line.contains("authority, gameplay"));
    }

    #[test]
    fn columns_are_aligned() {
        let entries = vec![
            RouteEntry {
                route_path: "/api/a".into(),
                handler_fn_name: "short".into(),
                protocol_name: "A".into(),
                kind: "Command".into(),
                surfaces: vec![],
            },
            RouteEntry {
                route_path: "/api/very/long/path".into(),
                handler_fn_name: "very_long_handler_name".into(),
                protocol_name: "B".into(),
                kind: "Query".into(),
                surfaces: vec![],
            },
        ];
        let output = format_route_table(&entries, "test@0.1", "default");
        let lines: Vec<&str> = output.lines().collect();
        // Header is line 0, blank is line 1, column header is line 2, data starts at 3
        let header_line = lines[2];
        let first_data = lines[3];
        let second_data = lines[4];

        // PATH column positions should match across all lines.
        let header_path_pos = header_line.find("PATH").unwrap();
        let first_path_pos = first_data.find("/api/a").unwrap();
        let second_path_pos = second_data.find("/api/very/long/path").unwrap();
        assert_eq!(header_path_pos, first_path_pos);
        assert_eq!(header_path_pos, second_path_pos);
    }
}
