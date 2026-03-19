# Galeon Engine

A Rust game engine with a Three.js renderer. The editor is an Electrobun desktop app with native GPU via `<electrobun-wgpu>`. Games built with it deploy to desktop (Electrobun) and web (WASM + Three.js in browser).

Dual licensed under AGPL-3.0 and a Commercial license with tiered royalties.

## Architecture

Rust owns all engine logic. TypeScript is only used where browser APIs require it (Three.js, DOM for the editor shell).

```
crates/
  engine-macros/     # Proc-macro crate (derives for ECS components, systems)
  engine/            # Core engine: ECS, scheduler, plugin API, data loading
  engine-three-sync/ # WASM bridge — serialized ECS snapshots → Three.js

packages/
  runtime/           # @galeon/runtime — invoke/events bridge (thin JS↔WASM glue)
  engine-ts/         # @galeon/engine-ts — Three.js sync consumer (reads WASM snapshots)
  shell/             # @galeon/shell — Godot-style editor UI (Solid.js, CSS Grid panels)
```

### Crate Dependency Graph

```
engine-macros (proc-macro, standalone)
      ↓
   engine (ECS, scheduler, plugins, data loading)
      ↓
engine-three-sync (WASM bridge — ECS → Three.js)
```

### TS Package Graph

TS packages are thin bridges, not logic owners.

```
@galeon/runtime (invoke/events bridge)
      ↓
@galeon/engine-ts (Three.js sync — reads WASM snapshots, updates scene graph)

@galeon/shell (editor UI — Solid.js panels around viewport)
```

### Editor

The editor is an **Electrobun desktop app** — full filesystem access, git, file watching, native GPU via `<electrobun-wgpu>`. Not a browser app.

### Game Deployment

Games built with Galeon can target:
- **Desktop**: Electrobun (same runtime as the editor)
- **Web**: WASM + Three.js + `<canvas>` in browser

### Editor Shell (Godot-style)

The shell is a web application. The viewport is a contained panel (not the window). Panels surround it: ECS inspector, profiler, console, asset browser. CSS Grid layout with resizable splits.

```
┌─ Window ─────────────────────────────────────────────┐
│  Menu Bar                                            │
├──────────┬────────────────────────────┬──────────────┤
│  ECS     │  <electrobun-wgpu>         │  Inspector   │
│  Browser │  or <canvas>               │  Components  │
│          │  (game viewport)           │  Properties  │
├──────────┴────────────────────────────┴──────────────┤
│  Console │ Profiler │ Assets                          │
└──────────────────────────────────────────────────────┘
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
- **Rust-first**: All engine logic in Rust. TS only for browser APIs.
- **License header**: Every source file starts with `// SPDX-License-Identifier: AGPL-3.0-only OR Commercial`
- **Package creation**: Always use `cargo init` / `bun init`, never create config files manually
- **Workspace deps**: Shared dependency versions go in root `Cargo.toml` `[workspace.dependencies]`
- **TS config**: All packages extend `tsconfig.base.json` via project references
- **Crate naming**: `galeon-engine-*` prefix for all crates
- **TS naming**: `@galeon/*` scope for all packages
- **Data format**: RON for game data, TOML for config
- **Docs**: Update `docs/guide/` and `CHANGELOG.md` with every PR
- **No competitor references**: In committed code/docs, describe Galeon on its own merits
- **Default branch**: `master`
