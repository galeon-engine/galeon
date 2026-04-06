// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

use std::env;
use std::fmt::Write;

use serde::Deserialize;

use crate::generate;

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
    #[allow(dead_code)]
    surfaces: Vec<String>,
}

pub fn run() -> Result<(), String> {
    let start_dir = env::current_dir().map_err(|e| format!("failed to read current dir: {e}"))?;
    let (json, protocol_version) = generate::inspect_routes_json(&start_dir)?;
    let entries: Vec<RouteEntry> =
        serde_json::from_str(&json).map_err(|e| format!("failed to parse route data: {e}"))?;
    print!("{}", format_route_table(&entries, &protocol_version));
    Ok(())
}

fn format_route_table(entries: &[RouteEntry], protocol_version: &str) -> String {
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

    writeln!(
        out,
        "{:<method_w$}  {:<path_w$}  {:<handler_w$}  REQUEST",
        "METHOD", "PATH", "HANDLER",
    )
    .unwrap();

    for entry in entries {
        let kind_label = entry.kind.to_lowercase();
        writeln!(
            out,
            "{:<method_w$}  {:<path_w$}  {:<handler_w$}  {} ({})",
            "POST", entry.route_path, entry.handler_fn_name, entry.protocol_name, kind_label,
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
        let output = format_route_table(&sample_entries(), "fixture-protocol@0.3.1");
        assert!(output.contains("Protocol: fixture-protocol@0.3.1 — 2 routes"));
    }

    #[test]
    fn table_contains_column_headers() {
        let output = format_route_table(&sample_entries(), "test@0.1");
        assert!(output.contains("METHOD"));
        assert!(output.contains("PATH"));
        assert!(output.contains("HANDLER"));
        assert!(output.contains("REQUEST"));
    }

    #[test]
    fn table_contains_route_rows() {
        let output = format_route_table(&sample_entries(), "test@0.1");
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
        let output = format_route_table(&sample_entries(), "test@0.1");
        let dispatch_pos = output.find("/api/fleet/dispatch").unwrap();
        let snapshot_pos = output.find("/api/fleet/snapshot").unwrap();
        assert!(dispatch_pos < snapshot_pos);
    }

    #[test]
    fn empty_table_shows_no_routes() {
        let output = format_route_table(&[], "test@0.1");
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
        let output = format_route_table(&entries, "test@0.1");
        assert!(output.contains("1 route\n"));
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
        let output = format_route_table(&entries, "test@0.1");
        let lines: Vec<&str> = output.lines().collect();
        // Header is line 0, blank is line 1, column header is line 2, data starts at 3
        let header_line = lines[2];
        let first_data = lines[3];
        let second_data = lines[4];

        // METHOD column positions should match across all lines
        let header_path_pos = header_line.find("PATH").unwrap();
        let first_path_pos = first_data.find("/api/a").unwrap();
        let second_path_pos = second_data.find("/api/very/long/path").unwrap();
        assert_eq!(header_path_pos, first_path_pos);
        assert_eq!(header_path_pos, second_path_pos);
    }
}
