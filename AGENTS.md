# GALEON ENGINE

Rust-first game engine with a Three.js renderer and an Electrobun editor shell. The engine core lives in Rust; TypeScript exists to bridge browser and desktop webview APIs.

## Rules

1. **Follow shiplog.** This repo is already configured for **shiplog** full mode in `.shiplog/mode.md`. Prefer issue-driven work, shiplog branch naming, ID-first commits, worklog comments, and PR timelines. Do not invent a parallel workflow.
2. **Use `fff` MCP tools for file search.** Use `mcp__fff__find_files`, `mcp__fff__grep`, and related `fff` tools for locating files or text instead of ad hoc shell search.
3. **Keep Galeon Rust-first.** Engine logic belongs in Rust. TypeScript should stay limited to renderer integration, DOM APIs, and editor shell concerns.

## Quick Reference

```bash
# Rust workspace
cargo check --workspace
cargo test --workspace
cargo clippy -- -D warnings
cargo fmt --check

# WASM / Three sync
cargo check --target wasm32-unknown-unknown -p galeon-engine-three-sync
wasm-pack build crates/engine-three-sync --target web

# TypeScript workspace
bun install
bun run check
bun run build
```

## Architecture

```
crates/
  engine-macros/         # Proc macros for ECS and protocol derives
  engine/                # Core engine: ECS, scheduler, plugins, data loading
  engine-three-sync/     # WASM bridge from ECS snapshots to Three.js consumers
  galeon-cli/            # CLI entrypoint
  protocol-consumer-test # Protocol/codegen integration coverage
  protocol-rename-test   # Rename/codegen regression coverage

packages/
  runtime/               # Thin JS <-> WASM invoke/events bridge
  render-core/           # Framework-neutral render snapshot contract
  three/                 # Imperative Three.js adapter
  r3f/                   # React Three Fiber adapter
  shell/                 # Editor shell package
```

### Dependency Rule

Keep dependencies flowing outward from Rust core to adapters:

```text
galeon-engine-macros
        ->
    galeon-engine
        ->
galeon-engine-three-sync
```

TypeScript packages are consumers and bridges, not owners of engine behavior:

```text
@galeon/runtime

@galeon/render-core
        ->
@galeon/three
        ->
@galeon/r3f

@galeon/shell
```

Do not move game-engine logic into TypeScript because it is convenient. If behavior matters for runtime correctness, keep it in Rust.

## Workspace Conventions

- Rust edition is `2024`.
- Every source file starts with `// SPDX-License-Identifier: AGPL-3.0-only OR Commercial`.
- Shared Rust dependency versions belong in root `Cargo.toml` under `[workspace.dependencies]`.
- TypeScript packages extend `tsconfig.base.json` and participate in the project-reference build.
- Rust crate names use the `galeon-engine-*` prefix where applicable.
- TypeScript packages use the `@galeon/*` scope.
- Use RON for game data and TOML for config unless the existing area clearly uses another format.
- Update `docs/guide/` and `CHANGELOG.md` when behavior or public workflow changes.
- Avoid competitor references in committed code or docs.
- **Prior-art research**: Before implementing any non-trivial system, study how battle-tested OSS libraries solve the same problem. If diverging, read their git history first to avoid repeating known mistakes. Findings go in the vault (`galeon-engine/`), never in committed code.
- The default branch is `master`.

## Editor And Runtime Notes

- The editor is an Electrobun desktop app, not a browser-only app.
- The viewport is a contained panel inside the shell, not the whole window.
- Desktop targets use Electrobun; web targets use WASM plus Three.js in the browser.
- Treat `packages/runtime`, `packages/render-core`, `packages/three`, and `packages/r3f` as thin adapters around Rust-owned state.

## JS Tooling

- Use `js_repl` for Node-backed JavaScript when you need a persistent REPL or top-level `await`.
- Use dynamic `import(...)` in `js_repl`; static top-level imports are unsupported there.
- Avoid direct access to `process.stdin`, `process.stdout`, or `process.stderr` from `js_repl`.

## Documentation Map

- `CLAUDE.md`: high-level repo guidance
- `docs/guide/ecs.md`: ECS model and constraints
- `docs/guide/plugins.md`: plugin architecture
- `docs/guide/protocol.md`: protocol model and codegen expectations
- `docs/guide/three-sync.md`: Rust-to-Three synchronization contract
- `docs/guide/data-loading.md`: asset/data loading behavior
- `docs/guide/game-loop.md`: runtime loop details
- `docs/guide/time.md`: engine time model
- `docs/guide/deadlines.md`: current milestone constraints
