# Galeon Engine

A Rust + TypeScript game engine for real-time naval strategy games. Dual licensed under AGPL-3.0 and a Commercial license with tiered royalties.

## Architecture

```
crates/
  engine-macros/     # Proc-macro crate (derives for ECS components)
  engine/            # Core ECS engine (serde, ron serialization)
  engine-three-sync/ # WASM bridge to Three.js (wasm-bindgen)

packages/
  runtime/           # @galeon/runtime — TS runtime layer
  engine-ts/         # @galeon/engine-ts — TS bindings (depends on runtime)
  shell/             # @galeon/shell — UI shell (Solid.js, deferred)
```

### Crate Dependency Graph

```
engine-macros (proc-macro, standalone)
      ↓
   engine (ECS core)
      ↓
engine-three-sync (WASM bridge)
```

### TS Package Graph

```
@galeon/runtime (standalone)
      ↓
@galeon/engine-ts (schedule runner)

@galeon/shell (standalone)
```

## Build Commands

### Rust

```bash
cargo check --workspace          # Type-check all crates
cargo test --workspace           # Run all tests
cargo clippy -- -D warnings      # Lint
cargo fmt --check                # Format check
```

### WASM

```bash
cargo check --target wasm32-unknown-unknown -p galeon-engine-three-sync
wasm-pack build crates/engine-three-sync --target web
```

### TypeScript

```bash
bun install          # Install dependencies
bun run check        # Type-check all packages (tsc --build)
```

## Conventions

- **Rust edition**: 2024
- **License header**: Every source file starts with `// SPDX-License-Identifier: AGPL-3.0-only OR Commercial`
- **Package creation**: Always use `cargo init` / `bun init`, never create config files manually
- **Workspace deps**: Shared dependency versions go in root `Cargo.toml` `[workspace.dependencies]`
- **TS config**: All packages extend `tsconfig.base.json` via project references
- **Crate naming**: `galeon-engine-*` prefix for all crates
- **TS naming**: `@galeon/*` scope for all packages
