# Publishing Galeon packages

> **Consumer quick start:** see the [README](../../README.md) for which packages
> to install and what stability to expect. This guide covers the release
> procedure for maintainers.

Galeon publishes **four Rust packages** to crates.io and **six TypeScript
packages** to npm. Everything else in the workspace is internal.

The npm packages in this guide are the checked-in workspace packages under
`packages/*`. They are not a separate external-only package surface.

## Publish surfaces

### Rust library crates (crates.io)

| Crate | Package name | Order |
|-------|-------------|-------|
| Macros | `galeon-engine-macros` | 1 — publish first |
| Core engine | `galeon-engine` | 2 — depends on macros |
| Three.js sync | `galeon-engine-three-sync` | 3 — depends on engine |

### CLI binary (crates.io)

| Artifact | Package name | Order |
|----------|--------------|-------|
| CLI install surface | `galeon-cli` | 4 — publish after the libraries and npm starter deps |

### TypeScript packages (npm)

| Package | npm name | Order |
|---------|----------|-------|
| Runtime | `@galeon/runtime` | 1 — publish first |
| Render core | `@galeon/render-core` | 2 — framework-neutral render contract |
| Three adapter | `@galeon/three` | 3 — depends on render-core |
| R3F adapter | `@galeon/r3f` | 4 — depends on render-core/three |
| Shell | `@galeon/shell` | 5 — no deps, last by convention |

### Not published

- `galeon-protocol-rename-test`, `galeon-protocol-consumer-test` — `publish = false` (integration tests)

## Versioning

All four Rust packages and all TypeScript packages move in **lockstep**. The Rust
workspace version lives in `Cargo.toml` → `[workspace.package] version` and is
inherited by publishable crates via `version.workspace = true`, including
`galeon-cli`. npm package versions are kept in sync manually. Internal
dependencies use exact pins (`=X.Y.Z`) to enforce lockstep.

### Version bump checklist

Run the bump script from the repo root:

```bash
bash scripts/bump-version.sh A.B.C
```

This updates all 9 files (15 edits) after verifying the current versions are
consistent, and rolls back if verification fails. `galeon-cli` inherits the
workspace version automatically, so it does not need a separate version edit.
The script supports prerelease and build metadata tags
(`0.4.0-alpha.1`, `0.4.0-alpha-1+build-7`).

The script edits these locations:

1. `Cargo.toml` → `[workspace.package] version = "A.B.C"`
2. `crates/engine/Cargo.toml` → `galeon-engine-macros = { …, version = "=A.B.C" }`
3. `crates/engine-three-sync/Cargo.toml` → `galeon-engine = { …, version = "=A.B.C" }`
4. `packages/runtime/package.json` → `"version": "A.B.C"`
5. `packages/render-core/package.json` → `"version": "A.B.C"`
6. `packages/three/package.json` → `"version": "A.B.C"` **and** `"@galeon/render-core": "=A.B.C"`
7. `packages/r3f/package.json` → `"version": "A.B.C"` **and** exact `@galeon/*` pins
8. `packages/shell/package.json` → `"version": "A.B.C"`

After running, manually update the changelog:

7. `CHANGELOG.md` → move `## Unreleased` items under `## [A.B.C]`

## Path dependencies and versions (Rust)

Workspace crates use **path + pinned version**:

```toml
galeon-engine-macros = { path = "../engine-macros", version = "=0.4.0" }
```

Cargo strips `path` for published tarballs.

## Local checks

### Rust

```bash
cargo publish -p galeon-engine-macros --dry-run
cargo publish -p galeon-cli --dry-run
```

`galeon-engine` and `galeon-engine-three-sync` dry-runs only pass after their
dependencies exist on crates.io at the pinned version.

To validate the supported installed-binary bootstrap flow from source, run:

```bash
bash tests/local-first-starter-smoke.sh
```

### TypeScript

```bash
bunx tsc --build                           # Build JS + declarations
npm pack --dry-run --workspace=packages/runtime
npm pack --dry-run --workspace=packages/render-core
npm pack --dry-run --workspace=packages/three
npm pack --dry-run --workspace=packages/r3f
npm pack --dry-run --workspace=packages/shell
```

At the repo root, `bun run check` and `bun run build` both run `tsc --build`
across these same checked-in workspace packages.

## Release procedure

1. Run `bash scripts/bump-version.sh A.B.C` (see version bump checklist above).
2. Update `CHANGELOG.md`, then commit: `git commit -am "release: vA.B.C"`
3. Tag: `git tag vA.B.C && git push origin master vA.B.C`
4. The **Release** workflow triggers automatically:
   - CI runs first (reused via `workflow_call`)
   - Crates publish in order with `cargo search` propagation polling
   - npm packages publish with skip-if-exists guards
   - `galeon-cli` publishes after the starter's crate/npm dependencies exist
   - Post-publish verification installs from registries, including the CLI starter flow
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
cd packages/runtime    && npm publish --access public && cd ../..
cd packages/render-core && npm publish --access public && cd ../..
cd packages/three      && npm publish --access public && cd ../..
cd packages/r3f        && npm publish --access public && cd ../..
cd packages/shell      && npm publish --access public && cd ../..
```

**CLI (after crates + npm packages):**

```bash
cargo publish -p galeon-cli
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

## Consumer guidance

Galeon is pre-1.0. All ten published artifacts move in lockstep &mdash; pick a
minor version and pin to it. Read the [changelog](../../CHANGELOG.md) on each
upgrade.

- Core engine crates are published and intended for evaluation and early use.
- `galeon-cli` is the supported install surface for scaffolding and codegen.
- `@galeon/render-core`, `@galeon/three`, and `@galeon/r3f` are the supported
  render adapter packages. The legacy `@galeon/engine-ts` compatibility
  re-export package was retired in `0.5.0` (see issue #209); previously
  published versions remain installable on npm but no further releases will
  be cut from this repo.
- `@galeon/shell` is published but experimental &mdash; expect churn.
- Prerelease tags (`alpha`, `beta`, `rc`) are published to both registries
  under the `alpha` npm dist-tag.

For the full stability statement, see the [README](../../README.md#stability).

## Package Surface Maintenance Rule

If the TypeScript package surface changes in a way contributors can see
(package added, removed, renamed, moved out of `packages/*`, or changed between
published and internal), update these surfaces in the same PR:

1. root `package.json` workspace entries
2. `tsconfig.base.json` path aliases
3. README architecture, TypeScript build, and public package sections
4. this publishing guide
5. CI/release workflow references if publishability changed
