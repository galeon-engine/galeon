<!-- SPDX-License-Identifier: AGPL-3.0-only OR Commercial -->

# Local-First Starter

`galeon new <name> --preset local-first` now scaffolds a minimal web starter
that can reach a first rendered frame without manual assembly.

For the full CLI surface, the non-runnable presets, and the current codegen
commands, see [cli-getting-started.md](cli-getting-started.md).

## What the Scaffold Includes

- `crates/domain` with a Rust-owned `StarterPlugin` that spawns the first
  renderable entity
- `crates/client` as a small `wasm-bindgen` wrapper around
  `galeon-engine-three-sync::WasmEngine`
- `client/` as a Vite + Three.js web app that consumes `@galeon/three` (the
  framework-neutral imperative adapter) and `@galeon/render-core` (the
  shared snapshot contract)
- root `package.json` scripts for `wasm`, `dev`, `build`, and `check`
- a generated project `README.md` that repeats the starter commands

The starter is intentionally narrow: one mesh, one camera, a small Three.js
scene, and a documented build loop. It is a first runnable path, not an editor
shell.

## Prerequisites

- Rust stable
- `wasm32-unknown-unknown` target
- `wasm-pack`
- Bun

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-pack
```

## Start the Starter

From the generated project root:

```bash
bun install
bun run dev
```

That flow:

1. builds `crates/client` to `client/pkg/` with `wasm-pack`
2. starts a Vite dev server rooted at `client/`
3. renders the Rust-owned starter entity through `RendererCache`

## Other Commands

```bash
bun run check
bun run build
```

- `check` rebuilds the WASM client and type-checks the web app
- `build` rebuilds the WASM client and emits a production Vite bundle

If you change Rust code while the dev server is already running, rerun:

```bash
bun run wasm
```

## Where To Extend It

- `crates/domain/src/lib.rs`
  Add Rust-owned gameplay state, systems, and starter entities
- `crates/client/src/lib.rs`
  Adjust the browser-facing WASM exports
- `client/src/main.ts`
  Change the camera, renderer setup, and scene-side presentation
- `crates/protocol/src/lib.rs`
  Define protocol types when the project grows beyond the starter path
