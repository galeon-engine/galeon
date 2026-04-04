# Publishing Galeon crates to crates.io

This repository publishes **three** Rust crates. Everything else in the workspace
is internal (tests, CLI tooling, or deferred).

## Publish surface

| Crate | Package name on crates.io | Order |
|-------|---------------------------|-------|
| Macros | `galeon-engine-macros` | 1 — publish first |
| Core engine | `galeon-engine` | 2 — depends on macros |
| Three.js sync | `galeon-engine-three-sync` | 3 — depends on engine |

## Not published

- `galeon-cli` — `publish = false` (deferred)
- `galeon-protocol-rename-test`, `galeon-protocol-consumer-test` — `publish = false` (integration tests)

JavaScript packages under `packages/` are **not** published to npm until separate
blockers are resolved (see project issues).

## Path dependencies and versions

Workspace crates that ship to crates.io use **path + pinned version** on each
other, for example:

```toml
galeon-engine-macros = { path = "../engine-macros", version = "=0.1.0" }
```

When you publish, Cargo strips `path` and keeps the version for consumers.
Keep these versions aligned with the `[package] version` of the dependency.

## Local checks

Cargo turns path dependencies into registry versions when it builds the publish
tarball. Until a dependency already exists on crates.io at the pinned version,
`cargo publish -p galeon-engine` and `cargo publish -p galeon-engine-three-sync`
fail during packaging (even with `--no-verify`).

**Always valid (CI runs this after tests):**

```bash
cargo publish -p galeon-engine-macros --dry-run
```

**After `galeon-engine-macros` is on crates.io at the pinned version:**

```bash
cargo publish -p galeon-engine --dry-run --no-verify
```

**After `galeon-engine` is on crates.io at the pinned version:**

```bash
cargo publish -p galeon-engine-three-sync --dry-run --no-verify
```

`--no-verify` skips the extracted-crate build step against the registry; you can
drop it once you want the stricter check.

After all three crates exist on crates.io for the current versions, you can run
the three commands back-to-back for a full preflight.

## Release procedure

1. Bump versions in all three `Cargo.toml` files (keep path-dep pins in sync).
2. Commit and tag (for example `v0.1.1` if all three share the same release).
3. Run the **Release** GitHub workflow (see `.github/workflows/release.yml`) or
   publish manually:

```bash
cargo publish -p galeon-engine-macros
# wait until the crate is visible on crates.io
cargo publish -p galeon-engine
cargo publish -p galeon-engine-three-sync
```

4. `galeon-engine-three-sync` needs the `wasm32-unknown-unknown` target for
   packaging; ensure it is installed (`rustup target add wasm32-unknown-unknown`).

## Authentication

- **CI / GitHub Actions:** store a crates.io token in the repository secret
  `CARGO_REGISTRY_TOKEN`.
- **Local:** `cargo login` or set `CARGO_REGISTRY_TOKEN` in the environment.
