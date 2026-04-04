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

All Rust crates and all TypeScript packages move in **lockstep**. Internal
dependencies use exact pins (`=0.1.0`) to enforce this.

When bumping versions:
1. Update `version` in all three `crates/*/Cargo.toml` and the workspace root.
2. Update `version` in all three `packages/*/package.json`.
3. Update dependency version pins between packages.

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

1. Bump versions in all `Cargo.toml` and `package.json` files (keep pins in sync).
2. Commit and tag (e.g. `v0.2.0`).
3. Run the **Release** GitHub workflow (`.github/workflows/release.yml`):
   - Choose target: `all`, `crates`, or `npm`.
   - Use `dry_run: true` first to validate.

### Manual publish (first time or fallback)

**Rust:**

```bash
cargo publish -p galeon-engine-macros
# wait ~45s for crates.io index
cargo publish -p galeon-engine
# wait ~45s
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
