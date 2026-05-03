# CLI

## Scaffold Engine Version Pin

The major.minor that scaffolded projects pin against (`galeon-engine = "X.Y"`)
is derived at build time from `crates/engine/Cargo.toml` by
`crates/galeon-cli/build.rs`. The build script resolves
`version.workspace = true` against the workspace root and writes
`pub(crate) const PUBLISHED_GALEON_ENGINE_VERSION: &str = "X.Y";` into the
CLI's build `OUT_DIR`; `crates/galeon-cli/src/templates.rs` includes it as the
single source of truth, so all template helpers
(`protocol_cargo_toml`, `domain_cargo_toml`, `server_cargo_toml`,
`local_first_client_cargo_toml`, `galeon_toml`) emit the same minor.

Bumping the workspace `version` (or the engine package's literal version) is
the only step needed to update the scaffold pin — the build script picks the
change up via `cargo:rerun-if-changed` on both manifests, and the
`template_dep_tests::published_constant_matches_engine_package_manifest`
unit test fails fast if the build-script output ever disagrees with the
engine package.

## Scaffold Smoke Checks

Use this check after changing `galeon new` templates or dependency pins. It
scaffolds one project for each preset outside the repository and runs
`cargo check --workspace` in each generated workspace.

```bash
tmp_dir="$(mktemp -d)"
repo_cargo="$PWD/Cargo.toml"

for preset in local-first hybrid server-authoritative; do
  project="check-$preset"
  (
    cd "$tmp_dir"
    cargo run --manifest-path "$repo_cargo" -p galeon-cli -- new "$project" --preset "$preset"
  )
  (
    cd "$tmp_dir/$project"
    cargo check --workspace
  )
done
```
