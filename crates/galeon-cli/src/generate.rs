// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use clap::{Args, Subcommand};
use serde::Deserialize;
use tempfile::TempDir;
use toml::Value;

#[derive(Subcommand, Clone, Debug)]
pub enum GenerateCommand {
    /// Emit TypeScript interfaces from the protocol crate
    Ts(GenerateArgs),
    /// Emit the collected protocol manifest as pretty JSON
    Manifest(GenerateArgs),
    /// Emit protocol descriptors as pretty JSON
    Descriptors(GenerateArgs),
    /// Emit filesystem-routed axum glue from the api/ directory
    Routes(GenerateArgs),
}

#[derive(Args, Clone, Debug)]
pub struct GenerateArgs {
    /// Override the output path
    #[arg(long)]
    pub out: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ArtifactKind {
    Ts,
    Manifest,
    Descriptors,
    Routes,
}

impl ArtifactKind {
    fn as_cli_arg(self) -> &'static str {
        match self {
            Self::Ts => "ts",
            Self::Manifest => "manifest",
            Self::Descriptors => "descriptors",
            Self::Routes => "routes",
        }
    }

    fn default_output_path(self, root: &Path) -> PathBuf {
        match self {
            Self::Ts => root.join("generated").join("types.ts"),
            Self::Manifest => root.join("generated").join("manifest.json"),
            Self::Descriptors => root.join("generated").join("descriptors.json"),
            Self::Routes => root.join("generated").join("routes.rs"),
        }
    }
}

#[derive(Debug)]
struct ProjectContext {
    root: PathBuf,
    protocol_dir: PathBuf,
    protocol_package_name: String,
    protocol_package_version: String,
    engine_dependency: Value,
}

impl ProjectContext {
    fn discover(start_dir: &Path) -> Result<Self, String> {
        let root = find_project_root(start_dir)?;
        let galeon_config = root.join("galeon.toml");
        let project_name = read_galeon_config(&galeon_config)?.project.name;
        if project_name.trim().is_empty() {
            return Err(format!(
                "project name in {} must not be empty",
                galeon_config.display()
            ));
        }
        let protocol_manifest = root.join("crates").join("protocol").join("Cargo.toml");
        if !protocol_manifest.exists() {
            return Err(format!(
                "expected Galeon protocol crate at {}",
                protocol_manifest.display()
            ));
        }

        let protocol_dir = protocol_manifest
            .parent()
            .ok_or_else(|| {
                format!(
                    "invalid protocol manifest path {}",
                    protocol_manifest.display()
                )
            })?
            .to_path_buf();
        let protocol_toml = read_toml_file(&protocol_manifest)?;
        let protocol_package = protocol_toml
            .get("package")
            .and_then(Value::as_table)
            .ok_or_else(|| {
                format!(
                    "protocol manifest {} is missing a [package] table",
                    protocol_manifest.display()
                )
            })?;
        let protocol_package_name = protocol_package
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                format!(
                    "protocol manifest {} is missing package.name",
                    protocol_manifest.display()
                )
            })?
            .to_string();
        let protocol_package_version = protocol_package
            .get("version")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                format!(
                    "protocol manifest {} is missing package.version",
                    protocol_manifest.display()
                )
            })?
            .to_string();
        let engine_dependency =
            resolve_engine_dependency(&root, &protocol_manifest, &protocol_toml)?;

        Ok(Self {
            root,
            protocol_dir,
            protocol_package_name,
            protocol_package_version,
            engine_dependency,
        })
    }

    fn protocol_version(&self) -> String {
        format!(
            "{}@{}",
            self.protocol_package_name, self.protocol_package_version
        )
    }
}

#[derive(Debug, Deserialize)]
struct GaleonConfig {
    project: GaleonProject,
}

#[derive(Debug, Deserialize)]
struct GaleonProject {
    name: String,
}

pub fn run(command: GenerateCommand) -> Result<PathBuf, String> {
    let start_dir = env::current_dir().map_err(|e| format!("failed to read current dir: {e}"))?;
    let (kind, args) = match command {
        GenerateCommand::Ts(args) => (ArtifactKind::Ts, args),
        GenerateCommand::Manifest(args) => (ArtifactKind::Manifest, args),
        GenerateCommand::Descriptors(args) => (ArtifactKind::Descriptors, args),
        GenerateCommand::Routes(args) => (ArtifactKind::Routes, args),
    };
    run_from_dir(kind, args.out.as_deref(), &start_dir)
}

fn run_from_dir(
    kind: ArtifactKind,
    out: Option<&Path>,
    start_dir: &Path,
) -> Result<PathBuf, String> {
    let context = ProjectContext::discover(start_dir)?;

    // For routes, scan the api/ directory and pass paths to the helper.
    let extra_args = if kind == ArtifactKind::Routes {
        let api_paths = scan_api_directory(&context.protocol_dir)?;
        let json = serde_json::to_string(&api_paths)
            .map_err(|e| format!("failed to serialize api paths: {e}"))?;
        vec!["--api-paths".to_string(), json]
    } else {
        vec![]
    };

    let artifact = execute_reflection_helper(&context, kind, &extra_args)?;
    let output_path = out
        .map(PathBuf::from)
        .unwrap_or_else(|| kind.default_output_path(&context.root));
    write_artifact(&output_path, artifact.as_bytes())?;
    Ok(output_path)
}

/// Walk the `api/` directory under the protocol crate and collect relative paths.
///
/// Returns paths relative to the protocol crate root (e.g., `"api/fleet/dispatch.rs"`),
/// using forward slashes on all platforms.
fn scan_api_directory(protocol_dir: &Path) -> Result<Vec<String>, String> {
    let api_dir = protocol_dir.join("src").join("api");
    if !api_dir.exists() {
        return Ok(vec![]);
    }
    let mut paths = Vec::new();
    collect_rs_files(&api_dir, &protocol_dir.join("src"), &mut paths)?;
    paths.sort();
    Ok(paths)
}

fn collect_rs_files(dir: &Path, base: &Path, out: &mut Vec<String>) -> Result<(), String> {
    let entries =
        fs::read_dir(dir).map_err(|e| format!("failed to read {}: {e}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("failed to read dir entry: {e}"))?;
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, base, out)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            let relative = path
                .strip_prefix(base)
                .map_err(|e| format!("path prefix error: {e}"))?;
            out.push(relative.to_string_lossy().replace('\\', "/"));
        }
    }
    Ok(())
}

fn execute_reflection_helper(
    context: &ProjectContext,
    kind: ArtifactKind,
    extra_args: &[String],
) -> Result<String, String> {
    let helper_dir = TempDir::new().map_err(|e| format!("failed to create helper dir: {e}"))?;
    let helper_manifest = helper_dir.path().join("Cargo.toml");
    let helper_src_dir = helper_dir.path().join("src");
    let helper_main = helper_src_dir.join("main.rs");

    fs::create_dir_all(&helper_src_dir)
        .map_err(|e| format!("failed to create helper source dir: {e}"))?;
    fs::write(
        &helper_manifest,
        helper_manifest_contents(context).as_bytes(),
    )
    .map_err(|e| format!("failed to write helper Cargo.toml: {e}"))?;
    fs::write(
        &helper_main,
        helper_main_source(&context.protocol_version()).as_bytes(),
    )
    .map_err(|e| format!("failed to write helper main.rs: {e}"))?;

    let mut cmd = Command::new("cargo");
    cmd.arg("run")
        .arg("--quiet")
        .arg("--manifest-path")
        .arg(&helper_manifest)
        .arg("--target-dir")
        .arg(context.root.join("target").join("galeon-generate"))
        .arg("--")
        .arg(kind.as_cli_arg());
    for arg in extra_args {
        cmd.arg(arg);
    }
    let output = cmd
        .current_dir(&context.root)
        .output()
        .map_err(|e| format!("failed to launch cargo for reflection helper: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = if !stderr.is_empty() {
            stderr
        } else if !stdout.is_empty() {
            stdout
        } else {
            format!("cargo exited with status {}", output.status)
        };
        return Err(format!(
            "reflection helper failed for `{}`: {}",
            kind.as_cli_arg(),
            detail
        ));
    }

    String::from_utf8(output.stdout)
        .map_err(|e| format!("reflection helper emitted non-utf8 output: {e}"))
}

fn helper_manifest_contents(context: &ProjectContext) -> String {
    let engine_dependency = render_toml_value(&context.engine_dependency);
    let protocol_path = render_toml_string(&context.protocol_dir.to_string_lossy());
    let protocol_package = render_toml_string(&context.protocol_package_name);

    format!(
        r#"[package]
name = "galeon-generate-helper"
version = "0.0.0"
edition = "2024"
publish = false

[dependencies]
galeon-engine = {engine_dependency}
serde_json = "1"
target_protocol = {{ path = {protocol_path}, package = {protocol_package} }}
"#
    )
}

fn helper_main_source(protocol_version: &str) -> String {
    let template = r#"// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

use std::process;

use galeon_engine::{
    generate_axum_routes, generate_descriptors, generate_typescript, resolve_routes,
    scan_api_routes, HandlerMeta, ProtocolManifest,
};
use target_protocol as _;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let kind = args.get(1).cloned().unwrap_or_else(|| {
        eprintln!("missing artifact kind");
        process::exit(2);
    });
    let manifest = ProtocolManifest::collect(__PROTOCOL_VERSION__);
    let output = match kind.as_str() {
        "ts" => generate_typescript(&manifest),
        "manifest" => manifest.to_json_pretty().expect("manifest json generation should succeed"),
        "descriptors" => serde_json::to_string_pretty(&generate_descriptors(&manifest))
            .expect("descriptor json generation should succeed"),
        "routes" => generate_routes(&args, &manifest),
        other => {
            eprintln!("unknown artifact kind: {other}");
            process::exit(2);
        }
    };
    print!("{output}");
}

fn generate_routes(args: &[String], manifest: &ProtocolManifest) -> String {
    // Parse --api-paths JSON from CLI args.
    let api_paths_json = args
        .windows(2)
        .find(|pair| pair[0] == "--api-paths")
        .map(|pair| pair[1].as_str())
        .unwrap_or("[]");
    let api_paths: Vec<String> =
        serde_json::from_str(api_paths_json).expect("failed to parse --api-paths JSON");
    let path_refs: Vec<&str> = api_paths.iter().map(String::as_str).collect();

    // T1: Scan filesystem paths into route entries.
    let scanned = scan_api_routes(&path_refs);

    // T2: Collect handler metadata from inventory.
    let handlers = HandlerMeta::collect_all();

    // T3: Resolve routes against handlers and manifest.
    let resolved = resolve_routes(&scanned, &handlers, manifest).unwrap_or_else(|errors| {
        for error in &errors {
            eprintln!("route resolution error: {error}");
        }
        process::exit(1);
    });

    // T4: Generate axum glue code.
    generate_axum_routes(&resolved, manifest).unwrap_or_else(|e| {
        eprintln!("route codegen error: {e}");
        process::exit(1);
    })
}
"#;
    template.replace(
        "__PROTOCOL_VERSION__",
        &render_rust_string(protocol_version),
    )
}

fn find_project_root(start_dir: &Path) -> Result<PathBuf, String> {
    for dir in start_dir.ancestors() {
        if dir.join("galeon.toml").exists() {
            return Ok(dir.to_path_buf());
        }
    }
    Err(format!(
        "could not find galeon.toml by walking up from {}",
        start_dir.display()
    ))
}

fn read_galeon_config(path: &Path) -> Result<GaleonConfig, String> {
    let source =
        fs::read_to_string(path).map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    toml::from_str(&source).map_err(|e| format!("failed to parse {}: {e}", path.display()))
}

fn read_toml_file(path: &Path) -> Result<Value, String> {
    let source =
        fs::read_to_string(path).map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    toml::from_str(&source).map_err(|e| format!("failed to parse {}: {e}", path.display()))
}

fn resolve_engine_dependency(
    project_root: &Path,
    protocol_manifest: &Path,
    protocol_toml: &Value,
) -> Result<Value, String> {
    let dependency = protocol_toml
        .get("dependencies")
        .and_then(Value::as_table)
        .and_then(|deps| deps.get("galeon-engine"))
        .cloned()
        .ok_or_else(|| {
            format!(
                "protocol manifest {} is missing dependencies.galeon-engine",
                protocol_manifest.display()
            )
        })?;

    if dependency
        .as_table()
        .and_then(|table| table.get("workspace"))
        .and_then(Value::as_bool)
        == Some(true)
    {
        let workspace_manifest = read_toml_file(&project_root.join("Cargo.toml"))?;
        let workspace_dependency = workspace_manifest
            .get("workspace")
            .and_then(|workspace| workspace.get("dependencies"))
            .and_then(Value::as_table)
            .and_then(|deps| deps.get("galeon-engine"))
            .cloned()
            .ok_or_else(|| {
                format!(
                    "workspace root {} does not define workspace.dependencies.galeon-engine",
                    project_root.join("Cargo.toml").display()
                )
            })?;
        return normalize_dependency_value(workspace_dependency, project_root);
    }

    let protocol_dir = protocol_manifest.parent().ok_or_else(|| {
        format!(
            "invalid protocol manifest path {}",
            protocol_manifest.display()
        )
    })?;
    normalize_dependency_value(dependency, protocol_dir)
}

fn normalize_dependency_value(value: Value, base_dir: &Path) -> Result<Value, String> {
    match value {
        Value::String(_) => Ok(value),
        Value::Table(mut table) => {
            if let Some(path_value) = table.get_mut("path") {
                let path_str = path_value.as_str().ok_or_else(|| {
                    "dependencies.galeon-engine.path must be a string".to_string()
                })?;
                let absolute = absolutize(base_dir, Path::new(path_str));
                *path_value = Value::String(absolute.to_string_lossy().into_owned());
            }
            table.remove("workspace");
            Ok(Value::Table(table))
        }
        other => Err(format!(
            "unsupported dependencies.galeon-engine format: {}",
            other.type_str()
        )),
    }
}

fn absolutize(base_dir: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base_dir.join(path)
    }
}

fn render_toml_value(value: &Value) -> String {
    match value {
        Value::String(value) => render_toml_string(value),
        Value::Integer(value) => value.to_string(),
        Value::Float(value) => value.to_string(),
        Value::Boolean(value) => value.to_string(),
        Value::Datetime(value) => value.to_string(),
        Value::Array(values) => {
            let rendered = values.iter().map(render_toml_value).collect::<Vec<_>>();
            format!("[{}]", rendered.join(", "))
        }
        Value::Table(table) => {
            let mut entries = table.iter().collect::<Vec<_>>();
            entries.sort_by(|(left, _), (right, _)| left.cmp(right));
            let rendered = entries
                .into_iter()
                .map(|(key, value)| format!("{key} = {}", render_toml_value(value)))
                .collect::<Vec<_>>();
            format!("{{ {} }}", rendered.join(", "))
        }
    }
}

fn render_toml_string(value: &str) -> String {
    serde_json::to_string(value).expect("json string rendering should succeed")
}

fn render_rust_string(value: &str) -> String {
    serde_json::to_string(value).expect("json string rendering should succeed")
}

fn write_artifact(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;
    }
    fs::write(path, bytes).map_err(|e| format!("failed to write {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    use tempfile::TempDir;

    fn repo_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf()
    }

    fn write_file(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, contents).unwrap();
    }

    fn fixture_protocol_manifest(engine_dependency: &str) -> String {
        format!(
            r#"[package]
name = "fixture-protocol"
version = "0.3.1"
edition = "2024"

[dependencies]
galeon-engine = {engine_dependency}
"#
        )
    }

    fn fixture_protocol_source() -> String {
        r#"// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

#[galeon_engine::command]
pub struct SpawnUnit {
    pub unit_id: u64,
}

#[galeon_engine::query]
pub struct GetWorldSnapshot;

#[galeon_engine::dto]
pub struct UnitSummary {
    pub unit_id: u64,
}
"#
        .to_string()
    }

    fn create_fixture_project() -> TempDir {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        let engine_path = repo_root().join("crates").join("engine");
        let engine_dependency = format!(
            "{{ path = {} }}",
            render_toml_string(&engine_path.to_string_lossy())
        );

        write_file(
            &root.join("galeon.toml"),
            r#"[project]
name = "fixture"
engine = "0.2"
preset = "local-first"
"#,
        );
        write_file(
            &root.join("crates").join("protocol").join("Cargo.toml"),
            &fixture_protocol_manifest(&engine_dependency),
        );
        write_file(
            &root
                .join("crates")
                .join("protocol")
                .join("src")
                .join("lib.rs"),
            &fixture_protocol_source(),
        );

        temp
    }

    #[test]
    fn discover_project_from_nested_directory() {
        let temp = create_fixture_project();
        let nested = temp.path().join("client").join("nested").join("deeper");
        fs::create_dir_all(&nested).unwrap();

        let context = ProjectContext::discover(&nested).unwrap();
        assert_eq!(context.root, temp.path());
        assert_eq!(context.protocol_package_name, "fixture-protocol");
        assert_eq!(context.protocol_version(), "fixture-protocol@0.3.1");
    }

    #[test]
    fn normalize_dependency_path_makes_relative_paths_absolute() {
        let base = Path::new("C:\\projects\\fixture");
        let value = toml::from_str::<Value>(r#"dep = { path = "../engine", version = "0.2.0" }"#)
            .unwrap()
            .get("dep")
            .cloned()
            .unwrap();
        let normalized = normalize_dependency_value(value, base).unwrap();
        let table = normalized.as_table().unwrap();
        let expected = absolutize(base, Path::new("../engine"));
        assert_eq!(
            table.get("path").and_then(Value::as_str),
            Some(expected.to_string_lossy().as_ref())
        );
        assert_eq!(table.get("version").and_then(Value::as_str), Some("0.2.0"));
    }

    #[test]
    fn generate_commands_write_expected_artifacts() {
        let temp = create_fixture_project();

        let ts_path = run_from_dir(ArtifactKind::Ts, None, temp.path()).unwrap();
        let manifest_path = run_from_dir(ArtifactKind::Manifest, None, temp.path()).unwrap();
        let descriptors_path = run_from_dir(ArtifactKind::Descriptors, None, temp.path()).unwrap();

        assert_eq!(ts_path, temp.path().join("generated").join("types.ts"));
        assert_eq!(
            manifest_path,
            temp.path().join("generated").join("manifest.json")
        );
        assert_eq!(
            descriptors_path,
            temp.path().join("generated").join("descriptors.json")
        );

        let typescript = fs::read_to_string(ts_path).unwrap();
        assert!(typescript.contains("export interface SpawnUnit"));
        assert!(typescript.contains("export interface UnitSummary"));

        let manifest = fs::read_to_string(manifest_path).unwrap();
        assert!(manifest.contains(r#""protocol_version": "fixture-protocol@0.3.1""#));
        assert!(manifest.contains(r#""name": "SpawnUnit""#));

        let descriptors = fs::read_to_string(descriptors_path).unwrap();
        assert!(descriptors.contains(r#""route": "/commands/spawn-unit""#));
        assert!(descriptors.contains(r#""name": "GetWorldSnapshot""#));
    }

    #[test]
    fn custom_out_path_overrides_default_output() {
        let temp = create_fixture_project();
        let custom = temp.path().join("artifacts").join("protocol-types.ts");

        let written = run_from_dir(ArtifactKind::Ts, Some(&custom), temp.path()).unwrap();

        assert_eq!(written, custom);
        assert!(written.exists());
        assert!(!temp.path().join("generated").join("types.ts").exists());
    }

    #[test]
    fn helper_manifest_uses_target_protocol_alias() {
        let temp = create_fixture_project();
        let context = ProjectContext::discover(temp.path()).unwrap();
        let manifest = helper_manifest_contents(&context);
        assert!(manifest.contains("target_protocol"));
        assert!(manifest.contains("fixture-protocol"));
    }

    #[test]
    fn run_requires_galeon_project_root() {
        let temp = TempDir::new().unwrap();
        let error = run_from_dir(ArtifactKind::Manifest, None, temp.path()).unwrap_err();
        assert!(error.contains("could not find galeon.toml"));
    }

    // -- Routes generation (T5) --

    fn fixture_routes_protocol_source() -> String {
        r#"// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

#[galeon_engine::command]
pub struct SpawnUnit {
    pub unit_id: u64,
}

#[galeon_engine::query]
pub struct GetWorldSnapshot;

#[galeon_engine::dto]
pub struct UnitSummary {
    pub unit_id: u64,
}

pub mod api {
    pub mod fleet {
        pub mod dispatch;
        pub mod snapshot;
    }
}
"#
        .to_string()
    }

    fn fixture_handler_dispatch() -> String {
        r#"// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

#[galeon_engine::handler]
pub fn dispatch_fleet(cmd: crate::SpawnUnit) -> Result<(), String> {
    let _ = cmd;
    Ok(())
}
"#
        .to_string()
    }

    fn fixture_handler_snapshot() -> String {
        r#"// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

#[galeon_engine::handler]
pub fn fleet_snapshot(query: crate::GetWorldSnapshot) -> Result<crate::UnitSummary, String> {
    let _ = query;
    Ok(crate::UnitSummary { unit_id: 0 })
}
"#
        .to_string()
    }

    fn fixture_helper_types() -> String {
        r#"// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

/// Shared types — this file is _-prefixed, so it must NOT become a route.
pub type FleetId = u64;
"#
        .to_string()
    }

    fn create_fixture_routes_project() -> TempDir {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        let engine_path = repo_root().join("crates").join("engine");
        let engine_dependency = format!(
            "{{ path = {} }}",
            render_toml_string(&engine_path.to_string_lossy())
        );

        write_file(
            &root.join("galeon.toml"),
            r#"[project]
name = "fixture-routes"
engine = "0.2"
preset = "server-authoritative"
"#,
        );
        write_file(
            &root.join("crates").join("protocol").join("Cargo.toml"),
            &fixture_protocol_manifest(&engine_dependency),
        );
        write_file(
            &root
                .join("crates")
                .join("protocol")
                .join("src")
                .join("lib.rs"),
            &fixture_routes_protocol_source(),
        );
        write_file(
            &root
                .join("crates")
                .join("protocol")
                .join("src")
                .join("api")
                .join("fleet")
                .join("dispatch.rs"),
            &fixture_handler_dispatch(),
        );
        write_file(
            &root
                .join("crates")
                .join("protocol")
                .join("src")
                .join("api")
                .join("fleet")
                .join("snapshot.rs"),
            &fixture_handler_snapshot(),
        );
        write_file(
            &root
                .join("crates")
                .join("protocol")
                .join("src")
                .join("api")
                .join("_types.rs"),
            &fixture_helper_types(),
        );

        temp
    }

    #[test]
    fn generate_routes_produces_axum_glue() {
        let temp = create_fixture_routes_project();

        let routes_path = run_from_dir(ArtifactKind::Routes, None, temp.path()).unwrap();

        assert_eq!(routes_path, temp.path().join("generated").join("routes.rs"));

        let routes = fs::read_to_string(routes_path).unwrap();

        // Header present.
        assert!(routes.contains("Auto-generated by Galeon Engine"));
        assert!(routes.contains("fixture-protocol@0.3.1"));

        // Command route is POST.
        assert!(routes.contains("\"/api/fleet/dispatch\""));
        assert!(routes.contains("routing::post(api_fleet_dispatch)"));
        assert!(routes.contains("dispatch_command_json"));
        assert!(routes.contains("\"SpawnUnit\""));

        // Query route is POST (all routes use POST to avoid unit-struct
        // vs empty-named-struct deserialization ambiguity).
        assert!(routes.contains("\"/api/fleet/snapshot\""));
        assert!(routes.contains("routing::post(api_fleet_snapshot)"));
        assert!(routes.contains("dispatch_query_json"));
        assert!(routes.contains("\"GetWorldSnapshot\""));

        // _types.rs must NOT appear as a route.
        assert!(!routes.contains("_types"));
        assert!(!routes.contains("api_types"));
    }

    #[test]
    fn generate_routes_no_api_directory_produces_empty_router() {
        // Use the basic fixture (no api/ directory).
        let temp = create_fixture_project();

        let routes_path = run_from_dir(ArtifactKind::Routes, None, temp.path()).unwrap();
        let routes = fs::read_to_string(routes_path).unwrap();

        assert!(routes.contains("Router::new()"));
        // No .route() calls — empty router.
        assert!(!routes.contains(".route("));
    }

    #[test]
    fn scan_api_directory_collects_rs_files() {
        let temp = create_fixture_routes_project();
        let protocol_dir = temp.path().join("crates").join("protocol");

        let paths = scan_api_directory(&protocol_dir).unwrap();

        // Should find dispatch.rs, snapshot.rs, and _types.rs (scanner skips _ later).
        assert!(paths.contains(&"api/fleet/dispatch.rs".to_string()));
        assert!(paths.contains(&"api/fleet/snapshot.rs".to_string()));
        assert!(paths.contains(&"api/_types.rs".to_string()));
    }
}
