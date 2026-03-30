<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset=".github/galeon-logo-dark.svg">
    <source media="(prefers-color-scheme: light)" srcset=".github/galeon-logo-light.svg">
    <img alt="Galeon Engine" src=".github/galeon-logo-dark.svg" width="480">
  </picture>
</p>

<p align="center">
  <strong>Rust logic. Three.js rendering. Ship everywhere.</strong>
</p>

<p align="center">
  <a href="https://github.com/galeon-engine/galeon/actions"><img src="https://img.shields.io/github/actions/workflow/status/galeon-engine/galeon/ci.yml?branch=master&style=flat-square&label=CI" alt="CI"></a>
  <a href="https://github.com/galeon-engine/galeon/blob/master/LICENSE-AGPL"><img src="https://img.shields.io/badge/license-AGPL--3.0%20%7C%20Commercial-blue?style=flat-square" alt="License"></a>
  <a href="https://doc.rust-lang.org/edition-guide/rust-2024/"><img src="https://img.shields.io/badge/rust-2024%20edition-orange?style=flat-square&logo=rust" alt="Rust 2024"></a>
  <a href="https://www.npmjs.com/package/three"><img src="https://img.shields.io/badge/renderer-Three.js-049EF4?style=flat-square&logo=three.js" alt="Three.js"></a>
</p>

---

Galeon is a game engine where all logic lives in Rust and rendering is delegated to [Three.js](https://threejs.org). The Rust core compiles to WASM for the browser and runs natively on desktop via [Electrobun](https://electrobun.dev). Instead of porting an entire renderer to the web, Galeon bridges its ECS to the most battle-tested 3D framework on the internet.

> **Status**: Pre-alpha. The ECS, game loop, protocol layer, and WASM-to-Three.js bridge are built and tested. Not yet suitable for production games.

## Why

Every existing engine makes the same trade-off on the web: port the whole renderer to WASM and accept the limitations (no multithreading, WebGL2 ceiling, huge bundles, no ecosystem access). Galeon doesn't.

|  | Bevy | Godot | Galeon |
|--|------|-------|--------|
| **Web renderer** | wgpu via WASM | OpenGL ES 3.0 only | Three.js (WebGL2 + WebGPU) |
| **Web bundle** | Large (full renderer) | 10-30 MB | Small (logic WASM + Three.js) |
| **Three.js ecosystem** | None | None | Full access |
| **Language** | Rust | GDScript / C# | Rust (logic), TS (rendering) |
| **Architecture** | ECS | Scene tree | ECS + frame extraction |
| **Editor** | None (prototyping) | Mature GUI | AI-driven, code-first |
| **License** | MIT / Apache-2.0 | MIT | AGPL-3.0 + Commercial |
| **Deterministic sim** | Manual setup | Limited | First-class (fixed timestep, virtual time) |

## Architecture

```
                    Rust (WASM)                         TypeScript
  ┌─────────────────────────────────┐     ┌──────────────────────────────┐
  │                                 │     │                              │
  │   ECS World                     │     │   Three.js Scene Graph       │
  │   ┌───────┐ ┌───────┐          │     │   ┌───────┐                  │
  │   │Archetype│ │Archetype│ ...    │     │   │ Mesh  │                  │
  │   │ A,B,C  │ │ A,D    │         │     │   │ Mesh  │ ...              │
  │   └───────┘ └───────┘          │     │   │ Mesh  │                  │
  │         │                       │     │   └───────┘                  │
  │         v                       │     │       ^                      │
  │   extract_frame()               │     │       │                      │
  │         │                       │     │   RendererCache              │
  │         v                       │     │   .applyFrame()              │
  │   FramePacket                   │     │       ^                      │
  │   [f32; 10] per entity ─────────┼────>┤       │                      │
  │   struct-of-arrays              │     │   Flat typed arrays          │
  │                                 │     │   (zero object allocation)   │
  └─────────────────────────────────┘     └──────────────────────────────┘
```

Rust owns entities, components, systems, scheduling, time, and game data. TypeScript only touches what the browser requires: the Three.js scene graph and DOM. The boundary is a flat `FramePacket` (parallel typed arrays), not serialized objects.

## Features

### ECS

- **Archetype storage** -- entities with the same component set are packed together in dense columns for cache-friendly iteration
- **Generational entities** -- use-after-despawn is a compile-time impossibility, not a runtime prayer
- **Zero-allocation queries** -- `world.query::<(&Position, &Velocity)>()` returns a lazy iterator, not a collected `Vec`
- **Filters** -- `With<T>`, `Without<T>`, tuple composition: `world.query_filtered::<&Pos, (With<Health>, Without<Dead>)>()`
- **Component mutation** -- `insert` and `remove` trigger archetype migration with edge-cached transitions (O(1) after the first)
- **Bundles** -- spawn up to 8 components at once with compile-time duplicate rejection
- **Resources** -- type-safe world singletons (`insert_resource`, `resource::<T>()`, `take_resource`)

### Simulation

- **Fixed timestep** -- accumulator-based, deterministic. Default 10 Hz (configurable). Lockstep-multiplayer-ready from day one
- **Virtual time** -- pause, 0-8x speed, max-delta clamping (death spiral prevention). Opt-in; zero overhead when absent
- **Stage-based scheduling** -- systems grouped into stages, executed in registration order
- **Plugin system** -- bundle systems + resources into reusable units: `engine.add_plugin(CombatPlugin)`

### Protocol Layer

- **Boundary abstraction** -- `#[command]`, `#[query]`, `#[event]`, `#[dto]` attribute macros mark types at the engine boundary
- **Auto-serde** -- macros derive Serialize/Deserialize, register metadata via `inventory`
- **Manifest generation** -- `ProtocolManifest::collect()` produces a machine-readable JSON schema of every protocol type, its fields, and doc comments
- **Transport-agnostic** -- same types work for local WASM calls, WebSocket networking, or server-client architectures

### WASM Bridge

- **Struct-of-arrays transport** -- 10 floats per entity (pos/rot/scale) in flat `Float32Array`, plus parallel visibility and handle arrays
- **Two-pass extraction** -- borrow-safe hot path that extracts render data without double-borrowing archetype columns
- **Debug snapshot** -- separate JSON tooling path for inspectors and profilers (never on the render hot path)
- **JS time control** -- `pause()`, `resume()`, `set_speed()` exposed to the host via `wasm-bindgen`

### Data

- **RON templates** -- load game data from `.ron` files. Filename is the lookup key, deserialization is validation
- **DataRegistry** -- directory-based loading with merge support

## Quick Start

```bash
# Clone
git clone https://github.com/galeon-engine/galeon.git
cd galeon

# Rust: check, test, lint
cargo check --workspace
cargo test --workspace
cargo clippy -- -D warnings

# WASM: verify it compiles
cargo check --target wasm32-unknown-unknown -p galeon-engine-three-sync

# TypeScript
bun install
bun run check
```

### Define a component

```rust
use galeon_engine::Component;

#[derive(Component)]
struct Position {
    x: f32,
    y: f32,
}

#[derive(Component)]
struct Velocity {
    dx: f32,
    dy: f32,
}
```

### Spawn entities and query them

```rust
use galeon_engine::{World, Component};

let mut world = World::new();

// Spawn
let e = world.spawn((Position { x: 0.0, y: 0.0 }, Velocity { dx: 1.0, dy: 0.0 }));

// Immutable query
for (pos, vel) in world.query::<(&Position, &Velocity)>() {
    println!("{}, {}", pos.x + vel.dx, pos.y + vel.dy);
}

// Mutable query with filter
use galeon_engine::query::With;
for pos in world.query_filtered_mut::<&mut Position, With<Velocity>>() {
    pos.x += 1.0;
}
```

### Build a plugin

```rust
use galeon_engine::{Engine, Plugin, World};

struct PhysicsPlugin;

impl Plugin for PhysicsPlugin {
    fn build(&self, engine: &mut Engine) {
        engine.add_system("simulate", "apply_velocity", apply_velocity);
    }
}

fn apply_velocity(world: &mut World) {
    for (pos, vel) in world.query_mut::<(&mut Position, &Velocity)>() {
        pos.x += vel.dx;
        pos.y += vel.dy;
    }
}

// Wire it up
let mut engine = Engine::new();
engine.add_plugin(PhysicsPlugin);
engine.tick(0.016); // 16ms frame
```

### Define protocol types

```rust
use galeon_engine::protocol;

#[galeon_engine::command]
/// Move a unit to a target position.
struct MoveUnit {
    unit_id: u64,
    target: [f32; 2],
}

#[galeon_engine::event]
struct UnitMoved {
    unit_id: u64,
    position: [f32; 2],
}

// Generate the manifest
let manifest = galeon_engine::manifest::ProtocolManifest::collect("1.0");
println!("{}", manifest.to_json_pretty());
```

## Project Structure

```
crates/
  engine-macros/        Proc-macro crate (#[derive(Component)], #[command], etc.)
  engine/               Core ECS, scheduler, plugins, game loop, protocol, data loading
  engine-three-sync/    WASM bridge -- frame extraction, typed array transport, JS bindings

packages/
  runtime/              @galeon/runtime -- version + future invoke/events bridge
  engine-ts/            @galeon/engine-ts -- Three.js RendererCache (consumes FramePackets)
  shell/                @galeon/shell -- editor shell (future)

docs/
  guide/                Architecture docs (ECS, plugins, game loop, time, three-sync, data)
```

### Crate dependency graph

```
engine-macros  (proc-macro, standalone)
      |
      v
   engine  (ECS, scheduler, plugins, data)
      |
      v
engine-three-sync  (WASM bridge, wasm-bindgen)
```

## Roadmap

Planned features tracked via [GitHub Issues](https://github.com/galeon-engine/galeon/issues):

- **TypeScript codegen** -- `galeon generate ts` CLI that reads the protocol manifest and emits typed TS clients with zero manual bindings ([#77](https://github.com/galeon-engine/galeon/issues/77))
- **Multi-API surfaces** -- generate different API shapes (game client, editor, server) from one Rust codebase ([#81](https://github.com/galeon-engine/galeon/issues/81))
- **Transport codegen** -- `galeon dev` / `galeon build --target server` for full dev + production pipelines ([#74](https://github.com/galeon-engine/galeon/issues/74))
- **2D rendering** -- Transform2D, SpriteHandle, orthographic SpriteRendererCache ([#38](https://github.com/galeon-engine/galeon/issues/38), [#39](https://github.com/galeon-engine/galeon/issues/39))
- **Input bridge** -- TS-to-WASM event forwarding for keyboard, mouse, gamepad ([#40](https://github.com/galeon-engine/galeon/issues/40))
- **Change detection** -- only sync what changed per frame ([#34](https://github.com/galeon-engine/galeon/issues/34))
- **Hot-reload** -- live RON file reloading during development ([#35](https://github.com/galeon-engine/galeon/issues/35))
- **Optional queries** -- `Option<&T>` for single-pass extraction ([#53](https://github.com/galeon-engine/galeon/issues/53))
- **Brandenburg** -- first-party reference RTS game ([#41](https://github.com/galeon-engine/galeon/issues/41))

## Vision

Most game engines ship an editor with a scene tree, an inspector, and a property panel. We think the future is different.

Game development is becoming **AI-driven and data-driven**. You describe what you want. The AI writes the code. You give feedback. The engine is the compiler and runtime, not a GUI you click through. The protocol manifest exists so that code generation -- whether by humans, templates, or LLMs -- has a machine-readable contract to work against.

Galeon is designed for this workflow: Rust as the source of truth, typed protocols as the boundary, code generation as the bridge, and Three.js as the renderer that already works everywhere.

## Deployment Targets

| Target | Runtime | Renderer |
|--------|---------|----------|
| **Web** | WASM (wasm-pack) | Three.js in browser (WebGL2 / WebGPU) |
| **Desktop** | Electrobun (native) | Three.js via Electrobun webview or native GPU |

## License

Galeon is dual-licensed:

- **[AGPL-3.0](LICENSE-AGPL)** -- free for open-source projects. Modifications must be shared under the same terms.
- **[Commercial](LICENSE-COMMERCIAL.md)** -- for closed-source games and applications. Free up to $100K gross revenue, tiered royalties above.

| Gross Revenue | Royalty |
|---------------|---------|
| Up to $100K | Free |
| $100K - $500K | 1% |
| $500K - $1M | 3% |
| Above $1M | 5% |

Every source file carries `// SPDX-License-Identifier: AGPL-3.0-only OR Commercial`.

## Contributing

Contributions are welcome. All contributed code is licensed under the dual AGPL-3.0 / Commercial license.

```bash
# Before submitting a PR
cargo fmt --check
cargo clippy -- -D warnings
cargo test --workspace
cargo check --target wasm32-unknown-unknown -p galeon-engine-three-sync
bun run check
```

See [CHANGELOG.md](CHANGELOG.md) for what has shipped.
