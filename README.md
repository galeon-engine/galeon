# Galeon Engine

A Rust game engine with a Three.js renderer.

Rust owns all engine logic. TypeScript is only used where browser APIs require it
(Three.js scene graph, DOM for the editor shell). Games deploy to desktop
([Tauri](https://tauri.app) or [Electrobun](https://electrobun.dev)) and web
(WASM + Three.js in the browser). Tauri is the fast path for shipping; Electrobun
is the option when you need native GPU via `<electrobun-wgpu>`.

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
- `SystemParam` trait with compile-time access conflict detection
- Stage-based scheduler with automatic command application between stages
- Fixed-timestep game loop (configurable Hz, defaults to 10 Hz for RTS)
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
- `WasmEngine` JS-facing handle: `tick()`, `extract_frame()`, `debug_snapshot()`
- `@galeon/engine-ts` &mdash; `RendererCache` syncs `FramePacket` to a Three.js scene graph
- Generational entity safety prevents stale object references
- Fallback geometry for missing assets

**CLI**
- `galeon new <name> --preset <preset>` scaffolds a complete game project
- Presets: `server-authoritative`, `local-first`, `hybrid`

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
    engine.add_system::<(QueryMut<'_, Score>,)>("update", "add_score", add_score);
    engine.world_mut().spawn((Score(0),));
    engine.tick(0.1);

    // Score is now 1 after one tick.
}
```

## Architecture

```
crates/
  engine-macros/       Proc-macro crate (#[derive(Component)], #[command], etc.)
  engine/              Core ECS, scheduler, protocol, data loading
  engine-three-sync/   WASM bridge — packed ECS snapshots to Three.js
  galeon-cli/          CLI binary (galeon new)

packages/
  runtime/             @galeon/runtime — JS/WASM glue
  engine-ts/           @galeon/engine-ts — Three.js RendererCache
  shell/               @galeon/shell — editor UI (Solid.js, planned)
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
struct CreateFleet { name: String, ship_count: u32 }

#[query]
struct GetFleet { id: u64 }

#[event(surface = "game")]
struct FleetCreated { id: u64, name: String }

#[dto]
struct FleetSummary { id: u64, name: String, ships: u32 }
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
