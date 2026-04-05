# Publishing Galeon packages

Galeon publishes **three Rust crates** to crates.io and **three TypeScript
packages** to npm. Everything else in the workspace is internal.

## Publish surfaces

### Rust crates (crates.io)

| Crate | Package name | Order |
|-------|-------------|-------|
| Macros | `galeon-engine-macros` | 1 — publish first |
| Core engine | `galeon-engine` | 2 — depends on macros |
| Three.js sync | `galeon-engine-three-sync` | 3 — depends on engine |

### TypeScript packages (npm)

| Package | npm name | Order |
|---------|----------|-------|
| Runtime | `@galeon/runtime` | 1 — publish first |
| Engine TS | `@galeon/engine-ts` | 2 — depends on runtime |
| Shell | `@galeon/shell` | 3 — no deps, last by convention |

### Not published

- `galeon-cli` — `publish = false` (deferred)
- `galeon-protocol-rename-test`, `galeon-protocol-consumer-test` — `publish = false` (integration tests)

## Versioning

All Rust crates and all TypeScript packages move in **lockstep**. The Rust
workspace version lives in `Cargo.toml` → `[workspace.package] version` and is
inherited by publishable crates via `version.workspace = true`. npm package
versions are kept in sync manually. Internal dependencies use exact pins
(`=X.Y.Z`) to enforce lockstep.

### Version bump checklist

Run the bump script from the repo root:

```bash
bash scripts/bump-version.sh A.B.C
```

This updates all 6 files (7 edits) after verifying the current versions are
consistent, and rolls back if verification fails. It supports prerelease and
build metadata tags (`0.2.0-alpha.1`, `0.2.0-alpha-1+build-7`).

The script edits these locations:

1. `Cargo.toml` → `[workspace.package] version = "A.B.C"`
2. `crates/engine/Cargo.toml` → `galeon-engine-macros = { …, version = "=A.B.C" }`
3. `crates/engine-three-sync/Cargo.toml` → `galeon-engine = { …, version = "=A.B.C" }`
4. `packages/runtime/package.json` → `"version": "A.B.C"`
5. `packages/engine-ts/package.json` → `"version": "A.B.C"` **and** `"@galeon/runtime": "=A.B.C"`
6. `packages/shell/package.json` → `"version": "A.B.C"`

After running, manually update the changelog:

7. `CHANGELOG.md` → move `## Unreleased` items under `## [A.B.C]`

## Path dependencies and versions (Rust)

Workspace crates use **path + pinned version**:

```toml
galeon-engine-macros = { path = "../engine-macros", version = "=0.1.0" }
```

Cargo strips `path` for published tarballs.

## Local checks

### Rust

```bash
cargo publish -p galeon-engine-macros --dry-run
```

`galeon-engine` and `galeon-engine-three-sync` dry-runs only pass after their
dependencies exist on crates.io at the pinned version.

### TypeScript

```bash
bunx tsc --build                           # Build JS + declarations
npm pack --dry-run --workspace=packages/runtime
npm pack --dry-run --workspace=packages/engine-ts
npm pack --dry-run --workspace=packages/shell
```

## Release procedure

1. Run `bash scripts/bump-version.sh A.B.C` (see version bump checklist above).
2. Update `CHANGELOG.md`, then commit: `git commit -am "release: vA.B.C"`
3. Tag: `git tag vA.B.C && git push origin master vA.B.C`
4. The **Release** workflow triggers automatically:
   - CI runs first (reused via `workflow_call`)
   - Crates publish in order with `cargo search` propagation polling
   - npm packages publish with skip-if-exists guards
   - Post-publish verification installs from registries
   - Evidence bundle uploaded as workflow artifact
   - GitHub Release created from the pushed tag, with prerelease tags marked as prereleases and the evidence markdown attached as a release asset

### Verify-only (manual dispatch)

Use `workflow_dispatch` with an explicit version input to re-verify an
already-published version without re-publishing. Installs from registries
and checks the artifacts work. Verify-only runs do **not** create or edit a
GitHub Release.

### Manual publish (first time or fallback)

**Rust:**

```bash
cargo publish -p galeon-engine-macros
# wait for crates.io index (CI uses cargo search polling)
cargo publish -p galeon-engine
cargo publish -p galeon-engine-three-sync
```

**npm (first publish — after this, use trusted publishing via CI):**

```bash
npm login
cd packages/runtime  && npm publish --access public && cd ../..
cd packages/engine-ts && npm publish --access public && cd ../..
cd packages/shell    && npm publish --access public && cd ../..
```

After the first publish, enable trusted publishing on npm for each package
(link to the `galeon-engine/galeon` GitHub repo). Subsequent releases use
OIDC provenance from GitHub Actions — no token needed.

## Authentication

### crates.io

- **CI:** `CARGO_REGISTRY_TOKEN` repository secret.
- **Local:** `cargo login` or set `CARGO_REGISTRY_TOKEN` in the environment.

### npm

- **CI:** Trusted publishing via OIDC (`id-token: write` permission in workflow).
  No `NPM_TOKEN` secret needed after initial setup.
- **Local (first publish only):** `npm login` with your npm account.
- **Scope:** The `@galeon` npm org owns the scope. Add team members via
  `npm org set galeon <user> developer`.
