# Galeon Engine

[![CI](https://github.com/galeon-engine/galeon/actions/workflows/ci.yml/badge.svg)](https://github.com/galeon-engine/galeon/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/galeon-engine)](https://crates.io/crates/galeon-engine)
[![cli](https://img.shields.io/crates/v/galeon-cli)](https://crates.io/crates/galeon-cli)
[![npm](https://img.shields.io/npm/v/@galeon/three)](https://www.npmjs.com/package/@galeon/three)
[![license](https://img.shields.io/crates/l/galeon-engine)](https://github.com/galeon-engine/galeon/blob/master/LICENSE-AGPL)

A Rust game engine with a Three.js renderer.

Rust owns all engine logic. TypeScript is only used where browser APIs require it
(Three.js scene graph, DOM for the editor shell). Games target desktop
([Tauri](https://tauri.app) or [Electrobun](https://electrobun.dev)) and web
(WASM + Three.js in the browser). Desktop shell integration is planned &mdash;
the engine itself is shell-agnostic.

> **Status:** Pre-release. The ECS, scheduler, protocol layer, and WASM bridge
> are functional and tested (350+ passing tests). API surface is stabilizing but
> may still change before 1.0.

## Features

**ECS**
- Archetype storage with generational entity handles
- Zero-allocation query iteration (single, 2-arity, 3-arity, mutable variants)
- `With<T>` / `Without<T>` query filters
- Per-component change detection (`ChangedIter`, `AddedIter`)
- Bundle spawning for up to 8-component tuples

**Systems and Scheduling**
- Parameterized systems: `fn(Res<T>, QueryMut<U>, Commands)` &mdash; no manual world access
- `SystemParam` trait with registration-time access conflict detection
- Stage-based scheduler with automatic command application between stages
- Fixed-timestep game loop with genre presets (10 Hz RTS, 20 Hz strategy, 30 Hz action, 60 Hz fast)
- Plugin API for modular engine extensions

**Resources and Events**
- Singleton resources via `Res<T>` / `ResMut<T>`
- Double-buffered typed events (`EventWriter<T>` / `EventReader<T>`)
- Deferred structural mutations via `Commands` (spawn, despawn, insert, remove)
- Deadline scheduler for UTC-based timed event firing (`Deadlines<T>`, `Clock` trait)

**Time**
- `VirtualTime` resource with pause, speed scaling (0&ndash;8&times;), and max-delta clamping
- `Engine::pause()`, `Engine::resume()`, `Engine::set_speed(scale)`

**Protocol and Codegen**
- Attribute macros: `#[command]`, `#[query]`, `#[event]`, `#[dto]`
- Automatic serde derives + compile-time `inventory` registration
- Surface scoping for multi-API projects
- `ProtocolManifest` &mdash; deterministic, git-diffable JSON/RON output
- TypeScript interface generation from Rust protocol types
- `HandlerRegistry` with typed local and remote (JSON boundary) dispatch

**Rendering**
- Built-in `Transform`, `Visibility`, `MeshHandle`, `MaterialHandle` components
- `FramePacket` struct-of-arrays extraction (full and incremental)
- Custom render channels via `ExtractToFloats` trait
- Change flags for incremental sync (`CHANGED_TRANSFORM`, `CHANGED_VISIBILITY`, etc.)

**WASM Bridge**
- `WasmEngine` JS-facing handle: `tick()`, `extract_frame()`, `debug_snapshot()`,
  `spawn_entity()`, `despawn_entity()`, `despawn_all_js_entities()`, `js_entity_count()`
- Dynamic entity spawn/despawn from JS with `JsSpawned` lifecycle guard
- `@galeon/render-core` defines the framework-neutral render snapshot contract
- `@galeon/three` syncs `FramePacket` to an imperative Three.js scene graph
- `@galeon/r3f` provides React Three Fiber provider/entities/hooks
- Generational entity safety prevents stale object references
- Fallback geometry for missing assets

**CLI**
- `cargo install --locked galeon-cli` is the supported way to install the
  scaffolding/codegen CLI
- `galeon new <name> --preset <preset>` scaffolds a preset-specific Galeon project
  and requires `name` to use lowercase letters, digits, and single hyphens
- Presets: `server-authoritative`, `local-first`, `hybrid`
- `local-first` is the only preset that currently scaffolds a runnable web
  starter with `bun run dev` / `bun run build`; see
  [docs/guide/local-first-starter.md](docs/guide/local-first-starter.md)
- `server-authoritative` and `hybrid` scaffold Rust-first workspace structure
  and client seams, not a ready-to-run shell
- `galeon generate <ts|manifest|descriptors|routes>` emits project artifacts
- `galeon routes` prints the effective route table for the current project

## Install CLI

```bash
cargo install --locked galeon-cli
galeon --help
```

`galeon-cli` moves in lockstep with the published Galeon engine release. An
installed `galeon-cli` scaffolds the matching Galeon crate/package version for
that release.

## CLI Getting Started

Today Galeon's CLI covers scaffolding, artifact generation, and route
inspection. It does not yet provide a universal `galeon dev`, `galeon run`, or
`galeon build` entrypoint.

For the current preset-by-preset flow:

- start with
  [docs/guide/cli-getting-started.md](docs/guide/cli-getting-started.md)
- use
  [docs/guide/local-first-starter.md](docs/guide/local-first-starter.md) for
  the runnable `local-first` starter
- use [docs/guide/protocol.md](docs/guide/protocol.md) for
  `galeon generate` outputs and protocol boundary details

Roadmap for the missing generic dev/watch workflow lives in
[#74](https://github.com/galeon-engine/galeon/issues/74) and
[#165](https://github.com/galeon-engine/galeon/issues/165).

## Quick Example

```rust
use galeon_engine::{Component, Engine, QueryMut};

#[derive(Component)]
struct Score(u32);

fn add_score(mut scores: QueryMut<'_, Score>) {
    for (_, s) in scores.iter_mut() {
        s.0 += 1;
    }
}

fn main() {
    let mut engine = Engine::new();
    engine.set_tick_rate(30.0); // 30 Hz for action games (default: 10 Hz)
    engine.add_system::<(QueryMut<'_, Score>,)>("update", "add_score", add_score);
    engine.world_mut().spawn((Score(0),));
    engine.tick(0.1);

    // Score is now 3 after three ticks (0.1s × 30 Hz = 3).
}
```

## Architecture

```
crates/
  engine-macros/       Proc-macro crate (#[derive(Component)], #[command], etc.)
  engine/              Core ECS, scheduler, protocol, data loading
  engine-three-sync/   WASM bridge — packed ECS snapshots to Three.js
  galeon-cli/          CLI binary (galeon new / generate / routes)

packages/
  runtime/             @galeon/runtime — JS/WASM glue (workspace + npm package)
  render-core/         @galeon/render-core — render packet contract (workspace + npm package)
  three/               @galeon/three — imperative Three.js adapter (workspace + npm package)
  r3f/                 @galeon/r3f — React Three Fiber adapter (workspace + npm package)
  shell/               @galeon/shell — editor UI package (workspace + npm package, experimental)
```

Crate dependency graph:

```
engine-macros (proc-macro, standalone)
      |
   engine
      |
engine-three-sync (WASM cdylib)
```

## Build

### Rust

```bash
cargo check --workspace                # Type-check all crates
cargo test --workspace                 # Run all tests
cargo clippy -- -D warnings            # Lint
cargo fmt --check                      # Format check
```

### WASM

```bash
cargo check --target wasm32-unknown-unknown -p galeon-engine-three-sync
wasm-pack build crates/engine-three-sync --target web
```

### TypeScript

```bash
bun install
bun run check    # Type-check all packages (tsc --build)
```

The Bun workspace in this repository is the checked-in `packages/*` tree:
`packages/runtime`, `packages/render-core`, `packages/three`,
`packages/r3f`, and `packages/shell`. These are the same
packages that publish to npm under the `@galeon/*` scope; they are not a separate
publish-only surface outside this checkout.

## Public Packages

Galeon publishes **four Rust packages** to [crates.io](https://crates.io) and
**five TypeScript packages** to [npm](https://www.npmjs.com).

The TypeScript packages listed here are also checked into this repository under
`packages/*` and are the packages targeted by the root Bun workspace commands.

### Rust crates

| Crate | crates.io | Description |
|-------|-----------|-------------|
| `galeon-engine-macros` | [![crates.io](https://img.shields.io/crates/v/galeon-engine-macros)](https://crates.io/crates/galeon-engine-macros) | Proc macros (`#[derive(Component)]`, `#[command]`, etc.) |
| `galeon-engine` | [![crates.io](https://img.shields.io/crates/v/galeon-engine)](https://crates.io/crates/galeon-engine) | Core ECS, scheduler, protocol, data loading |
| `galeon-engine-three-sync` | [![crates.io](https://img.shields.io/crates/v/galeon-engine-three-sync)](https://crates.io/crates/galeon-engine-three-sync) | WASM bridge (ECS snapshots &rarr; Three.js) |

### CLI binary

| Package | crates.io | Description |
|---------|-----------|-------------|
| `galeon-cli` | [![crates.io](https://img.shields.io/crates/v/galeon-cli)](https://crates.io/crates/galeon-cli) | Install surface for `galeon new`, `galeon generate`, and `galeon routes` |

### TypeScript packages

| Package | npm | Description |
|---------|-----|-------------|
| `@galeon/runtime` | [![npm](https://img.shields.io/npm/v/@galeon/runtime)](https://www.npmjs.com/package/@galeon/runtime) | JS &harr; WASM glue |
| `@galeon/render-core` | [![npm](https://img.shields.io/npm/v/@galeon/render-core)](https://www.npmjs.com/package/@galeon/render-core) | Framework-neutral render snapshot contract |
| `@galeon/three` | [![npm](https://img.shields.io/npm/v/@galeon/three)](https://www.npmjs.com/package/@galeon/three) | Imperative Three.js adapter |
| `@galeon/r3f` | [![npm](https://img.shields.io/npm/v/@galeon/r3f)](https://www.npmjs.com/package/@galeon/r3f) | React Three Fiber adapter |
| `@galeon/shell` | [![npm](https://img.shields.io/npm/v/@galeon/shell)](https://www.npmjs.com/package/@galeon/shell) | Editor UI package (experimental) |

### Not published

The following workspace members are internal and not published to any registry:

- `galeon-protocol-rename-test`, `galeon-protocol-consumer-test` &mdash; integration test crates

## Versioning

All four Rust packages and six TypeScript packages move in **lockstep**
&mdash; every release bumps all ten published artifacts to the same version
number.

### Pre-1.0 policy

Galeon follows [Semantic Versioning 2.0.0](https://semver.org/). During the
pre-1.0 phase:

- **Minor bumps** (`0.3 &rarr; 0.4`) may contain breaking API changes.
- **Patch bumps** (`0.4.0 &rarr; 0.4.1`) are backward-compatible bug fixes and
  additions.
- **Prerelease tags** (`0.4.0-alpha.1`, `0.4.0-beta.1`, `0.4.0-rc.1`) are
  published to crates.io and npm under the `alpha` dist-tag. Use these
  preview upcoming releases.

### How to depend on Galeon

```toml
# In your Cargo.toml — matches any 0.4.x release
galeon-engine = "0.4"
```

```json
// In your package.json — matches any 0.4.x release
"@galeon/three": "^0.4.0"
```

```bash
# Install the matching Galeon CLI release
cargo install --locked galeon-cli
```

See [docs/guide/publishing.md](docs/guide/publishing.md) for the full release
procedure and version bump checklist.

## Stability

Galeon is **pre-1.0 software** under active development. Here is what that
means for adopters:

**What you can rely on today:**
- The core engine crates (`galeon-engine`, `galeon-engine-macros`) are published,
  tested (350+ tests), and intended for evaluation and early adoption.
- `galeon-cli` is published and supported for project scaffolding, protocol
  artifact generation, and route inspection.
- The framework-neutral render contract and both Three.js host adapters are
  published as `@galeon/render-core`, `@galeon/three`, and `@galeon/r3f`.
- The ECS, scheduler, protocol layer, and WASM bridge are functional and
  cover real use cases.
- Lockstep versioning means all packages stay in sync &mdash; no version matrix to
  manage.

**What may still change:**
- Public API signatures may change between minor versions (`0.x &rarr; 0.y`).
- The editor shell (`@galeon/shell`) is scaffolded but not feature-complete.
- CLI commands and codegen output formats are still evolving.

**How to upgrade safely:**
- Pin to a specific minor range (e.g., `"0.4"` in Cargo, `"^0.4.0"` in npm).
- Read the [changelog](CHANGELOG.md) before upgrading &mdash; breaking changes are
  always documented.
- Prerelease tags (`alpha`, `beta`, `rc`) let you test upcoming versions before
  they go stable.

## System Parameters

| Parameter | Access | Description |
|-----------|--------|-------------|
| `Res<T>` | Shared read | Singleton resource |
| `ResMut<T>` | Exclusive write | Mutable singleton resource |
| `Query<T>` | Shared read | Iterate entities with component `T` |
| `QueryMut<T>` | Exclusive write | Mutably iterate entities with component `T` |
| `Commands` | Deferred | Spawn, despawn, insert, remove (applied between stages) |
| `EventWriter<T>` | Write | Send events for the next tick |
| `EventReader<T>` | Read | Read events from the previous tick |

Tuple parameters up to 8-arity are supported. Conflict detection runs at
system registration time &mdash; overlapping `Res<T>` + `ResMut<T>` in the same
system panics immediately, not at runtime.

## Protocol Macros

```rust
use galeon_engine::*;

#[command]
struct CreateUnit { name: String, count: u32 }

#[query]
struct GetUnits { id: u64 }

#[event(surface = "game")]
struct UnitSpawned { id: u64, name: String }

#[dto]
struct UnitSummary { id: u64, name: String, total: u32 }
```

These generate `Serialize`/`Deserialize` derives, `ProtocolMeta` impls, and
`inventory` registration. Collect them at runtime with
`ProtocolManifest::collect("1.0.0")` and generate TypeScript interfaces with
`generate_typescript(&manifest)`.

## License

Dual licensed under [AGPL-3.0](LICENSE-AGPL) and a
[Commercial License](LICENSE-COMMERCIAL.md) with tiered royalties.

| Gross Revenue | Royalty |
|---------------|---------|
| Up to $100K | Free |
| $100K &ndash; $500K | 1% above $100K |
| $500K &ndash; $1M | 3% above $500K |
| Above $1M | 5% above $1M |

Open-source projects can use Galeon under AGPL-3.0 at no cost.
Commercial projects that want to keep their source proprietary need the
Commercial License.

See [LICENSE-AGPL](LICENSE-AGPL) and [LICENSE-COMMERCIAL.md](LICENSE-COMMERCIAL.md)
for full terms.
