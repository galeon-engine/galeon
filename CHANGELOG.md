# Changelog

All notable changes to the Galeon Engine are documented here.

Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Versioning follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Per-component change detection with tick tracking
  ([#14](https://github.com/galeon-engine/galeon/issues/14))
- `World::query_changed::<T>(since)` — query only components modified after a tick
  ([#14](https://github.com/galeon-engine/galeon/issues/14))
- `World::query_added::<T>(since)` — query only components added after a tick
  ([#14](https://github.com/galeon-engine/galeon/issues/14))
- `World::current_tick()` — read the current ECS tick
  ([#14](https://github.com/galeon-engine/galeon/issues/14))
- `World::component_added_tick::<T>(entity)` — inspect when a component was added
  ([#14](https://github.com/galeon-engine/galeon/issues/14))
- `World::component_changed_tick::<T>(entity)` — inspect when a component last changed
  ([#14](https://github.com/galeon-engine/galeon/issues/14))
- Incremental extraction with change flags in `FramePacket`
  ([#14](https://github.com/galeon-engine/galeon/issues/14))
- Render extraction pipeline: `Transform`, `Visibility`, `MeshHandle`,
  `MaterialHandle` components with flat array layout for typed-buffer transport
  ([#15](https://github.com/galeon-engine/galeon/issues/15))
- `FramePacket` struct-of-arrays for WASM render hot path (10-float transform
  stride, parallel entity/visibility/mesh/material arrays)
  ([#15](https://github.com/galeon-engine/galeon/issues/15))
- `extract_frame(&World)` extraction system with two-pass borrow-split pattern
  ([#15](https://github.com/galeon-engine/galeon/issues/15))
- `WasmEngine` (tick + extract) and `WasmFramePacket` (getter-based flat array
  access) WASM bindings
  ([#15](https://github.com/galeon-engine/galeon/issues/15))
- `RendererCache` in `@galeon/engine-ts` — Three.js scene graph sync from
  extraction tables with create/update/remove lifecycle
  ([#15](https://github.com/galeon-engine/galeon/issues/15))
- `DebugSnapshot` tooling path — JSON serialisation of render-facing world
  state, separate from the hot render path
  ([#15](https://github.com/galeon-engine/galeon/issues/15))
- `docs/guide/three-sync.md` documenting the render extraction hot-path
  contract ([#15](https://github.com/galeon-engine/galeon/issues/15))

### Changed

- Component storage now uses typed sparse sets (`Vec<T>`) instead of
  type-erased `Vec<Box<dyn Any>>`, eliminating per-component heap allocation
  and per-entity runtime downcasts on all hot paths
  ([#12](https://github.com/galeon-engine/galeon/issues/12))

### Added

- `Engine` struct owning `World` + `Schedule` with a fluent builder API
  (`add_system`, `add_plugin`, `insert_resource`) and `tick`/`run_once`
  execution methods ([#8](https://github.com/galeon-engine/galeon/issues/8))
- `Plugin` trait (`fn build(&self, engine: &mut Engine)`) for bundling systems
  and resources into reusable units
  ([#8](https://github.com/galeon-engine/galeon/issues/8))
- `World::try_resource<T>()` — non-panicking resource probe returning
  `Option<&T>`
  ([#8](https://github.com/galeon-engine/galeon/issues/8))
- `docs/guide/plugins.md` — guide covering the builder API and plugin system
  ([#8](https://github.com/galeon-engine/galeon/issues/8))
- Fixed-step game loop with time accumulator (default 10 Hz for RTS)
  ([#6](https://github.com/galeon-engine/galeon/issues/6))
- RON data loading: `UnitTemplate`, `UnitStats`, `DataRegistry` for loading
  game data from `.ron` files
  ([#6](https://github.com/galeon-engine/galeon/issues/6))
- Minimal ECS core: Entity (generational indices), SparseSet component storage,
  World (spawn/despawn/query), typed Resources, stage-based Schedule
  ([#3](https://github.com/galeon-engine/galeon/issues/3))
- `#[derive(Component)]` macro with real trait implementation
- Cargo workspace with 3 crates: `engine-macros`, `engine`, `engine-three-sync`
  ([#1](https://github.com/galeon-engine/galeon/issues/1))
- Bun workspace with 3 TS packages: `@galeon/runtime`, `@galeon/engine-ts`,
  `@galeon/shell`
  ([#1](https://github.com/galeon-engine/galeon/issues/1))
- WASM bridge (`engine-three-sync`) with `wasm-bindgen` version export
- Dual license: AGPL-3.0 + Commercial with tiered royalties
- GitHub Actions CI: Rust (fmt, clippy, test, WASM check) + TypeScript (tsc)
