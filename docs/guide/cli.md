# CLI

## Scaffold Engine Version Pin

The major.minor that scaffolded projects pin against (`galeon-engine = "X.Y"`)
is derived at build time by `crates/galeon-cli/build.rs` and exposed as
`pub(crate) const PUBLISHED_GALEON_ENGINE_VERSION: &str = "X.Y";`.
`crates/galeon-cli/src/templates.rs` includes it as the single source of truth,
so all template helpers (`protocol_cargo_toml`, `domain_cargo_toml`,
`server_cargo_toml`, `local_first_client_cargo_toml`, `galeon_toml`) emit the
same minor.

The build script handles two source contexts:

- **In-workspace builds** (dev, CI, the source side of `cargo publish`):
  resolves the engine package's effective version from `crates/engine/Cargo.toml`
  (handling `version.workspace = true` against the workspace root) and
  cross-checks against `CARGO_PKG_VERSION`. A mismatch fails the build before
  the CLI can ship with a desynced engine pin. `cargo:rerun-if-changed`
  covers both manifests so workspace or engine-package edits re-fire the
  script.
- **Published-tarball builds** (`cargo install galeon-cli`): the engine
  manifest is not packaged, so the build script falls back to
  `CARGO_PKG_VERSION`. Cargo expands `version.workspace = true` to a literal
  at `cargo publish` time, so this is the resolved workspace version — i.e.
  the engine version this CLI release was paired with.

Bumping the workspace `version` is the only step needed to update the scaffold
pin. The
`template_dep_tests::published_constant_matches_engine_package_manifest` unit
test re-derives the engine major.minor independently and fails fast if the
build-script output ever disagrees with the engine package, catching
build-script regressions in CI.

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
