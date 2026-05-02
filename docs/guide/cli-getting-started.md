<!-- SPDX-License-Identifier: AGPL-3.0-only OR Commercial -->

# CLI Getting Started

This guide documents the current `galeon-cli` surface as it exists today.
Galeon can scaffold projects, generate protocol artifacts, and inspect resolved
routes. It does not yet expose a universal `galeon dev`, `galeon run`, or
`galeon build` command.

## Install The CLI

```bash
cargo install --locked galeon-cli
galeon --help
```

`galeon-cli` moves in lockstep with the Galeon release it scaffolds. An
installed `galeon-cli X.Y.x` emits a project wired to Galeon `X.Y.x`, not a
hardcoded older template version.

For prereleases, install the exact version you want to evaluate:

```bash
cargo install --locked galeon-cli --version 0.3.0-alpha.1
```

## Current Commands

```bash
galeon new <name> --preset <server-authoritative|local-first|hybrid>
galeon generate ts
galeon generate manifest
galeon generate descriptors
galeon generate routes
galeon routes
```

- `galeon new` creates a new project directory under the current working
  directory
- `galeon generate ...` runs inside an existing Galeon project and writes files
  under `generated/` by default
- `galeon routes` prints the effective route table for the current project

Project names for `galeon new` are intentionally strict: use lowercase ASCII
letters, digits, and single hyphens, start with a letter, and avoid reserved
Windows names such as `aux` and `con`. Example: `my-game-2`.

`galeon generate` and `galeon routes` both walk up from the current working
directory until they find `galeon.toml`, so you can run them from the project
root or from a nested crate directory.

## Pick A Preset

| Preset | What `galeon new` creates today | Ready-to-run path |
|--------|---------------------------------|-------------------|
| `local-first` | `crates/protocol`, `crates/domain`, `crates/client`, `client/`, root `package.json`, and a generated starter `README.md` | Yes. Run `bun install`, then `bun run dev`. |
| `server-authoritative` | `crates/protocol`, `crates/domain`, `crates/server`, `crates/db`, `docker-compose.yml`, and a placeholder `client/.gitkeep` | No. This preset scaffolds project structure only. |
| `hybrid` | `crates/protocol`, `crates/domain`, `crates/server`, and a placeholder `client/.gitkeep` | No. This preset scaffolds project structure only. |

Only `local-first` currently includes a documented starter loop. The other two
presets give you the Rust workspace layout and expected crate boundaries, but
you still add the runtime shell, transport glue, and app-specific boot flow.

## Local-First: First Running Project

Use `local-first` when you want the shortest path to a first rendered frame:

```bash
cargo install --locked galeon-cli
galeon new my-game --preset local-first
cd my-game
bun install
bun run dev
```

That flow:

1. creates a Rust-owned `StarterPlugin` in `crates/domain`
2. creates a `crates/client` WASM wrapper around
   `galeon-engine-three-sync::WasmEngine`
3. creates a `client/` Vite + Three.js app that consumes `@galeon/three`
   (the imperative render adapter) and `@galeon/render-core`
4. builds `crates/client` into `client/pkg/` with `wasm-pack`
5. starts the generated web starter through Vite

The generated project root also includes a starter README with the same
commands. For the detailed starter workflow, see
[local-first-starter.md](local-first-starter.md).

## Server-Authoritative And Hybrid: What Is Still Missing

`server-authoritative` and `hybrid` are intentionally more skeletal today.
After `galeon new`, you have the crate layout and config, but you do not yet get
any of the following by default:

- a generated browser client or desktop shell
- a universal `galeon dev` / `galeon run` command
- a watch loop that rebuilds codegen and launches runtime pieces for you

For these presets, the next step is to add your own server/runtime entrypoint,
client shell, and transport boundary on top of the scaffolded `protocol`,
`domain`, and `server` crates.

## Artifact Generation After Scaffolding

Inside any Galeon project, the CLI can emit protocol and transport artifacts:

| Command | Default output |
|---------|----------------|
| `galeon generate ts` | `generated/types.ts` |
| `galeon generate manifest` | `generated/manifest.json` |
| `galeon generate descriptors` | `generated/descriptors.json` |
| `galeon generate routes` | `generated/routes.rs` |

Use `--out <path>` on any `galeon generate` subcommand to override the default
destination. `galeon routes` is read-only: it prints the resolved route table
instead of writing a file.

For the protocol model behind these outputs, see
[protocol.md](protocol.md).

## Not Here Yet

The missing generic workflow is intentional roadmap, not a hidden command:

- [#74](https://github.com/galeon-engine/galeon/issues/74) tracks a future
  `galeon dev` / `galeon build` transport-aware flow
- [#165](https://github.com/galeon-engine/galeon/issues/165) tracks a future
  `galeon dev` watch mode for filesystem API autogen

Until those land, treat Galeon's CLI as a project scaffold plus artifact/codegen
tooling, with the `local-first` preset as the only built-in runnable starter.
