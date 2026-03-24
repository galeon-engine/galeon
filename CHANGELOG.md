# Changelog

All notable changes to the Galeon Engine are documented here.

Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Versioning follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed
- **Queries return lazy iterators instead of `Vec`** — `query()`, `query_mut()`, `query2()`, `query2_mut()` now return zero-allocation iterator structs that borrow directly from the sparse set ([#11](https://github.com/galeon-engine/galeon/issues/11))

### Added
- `query3()` and `query3_mut()` — three-component lazy queries ([#11](https://github.com/galeon-engine/galeon/issues/11))
- `QueryIter`, `QueryIterMut`, `Query2Iter`, `Query2MutIter`, `Query3Iter`, `Query3MutIter` iterator types ([#11](https://github.com/galeon-engine/galeon/issues/11))

- Archetype storage core types: `ArchetypeId`, `ArchetypeLayout`, `Column<T>`,
  `AnyColumn` trait, `Archetype`, `ArchetypeStore`, and edge cache for O(1)
  archetype transitions
  ([#27](https://github.com/galeon-engine/galeon/issues/27))
- `EntityMeta` + `EntityMetaStore` — location-aware entity allocator tracking
  archetype ID and row for O(1) entity lookup
  ([#27](https://github.com/galeon-engine/galeon/issues/27))

### Changed

- `Component` trait now requires `Send + Sync + 'static` (previously only
  `'static`), preparing for thread-safe archetype storage
  ([#27](https://github.com/galeon-engine/galeon/issues/27))

- Virtual time resource: pause, speed scaling (0–8×), and max-delta clamping
  to prevent death spirals. Opt-in via `VirtualTime` resource; backward
  compatible when absent
  ([#13](https://github.com/galeon-engine/galeon/issues/13))
- `Engine::pause()`, `Engine::resume()`, `Engine::set_speed(scale)` convenience
  API with lazy `VirtualTime` insertion
  ([#13](https://github.com/galeon-engine/galeon/issues/13))
- `WasmEngine::pause()`, `resume()`, `set_speed()`, `is_paused()` WASM bindings
  for JS host time control
  ([#13](https://github.com/galeon-engine/galeon/issues/13))
- `docs/guide/time.md` — virtual time guide
  ([#13](https://github.com/galeon-engine/galeon/issues/13))
- Protocol manifest: `ProtocolManifest::collect()` gathers all protocol items
  into a machine-readable schema with `manifest_version`, `protocol_version`,
  field-level detail, and doc comments. JSON + RON serialization. Uses
  `inventory` for distributed static registration
  ([#47](https://github.com/galeon-engine/galeon/issues/47))
- Protocol attribute macros: `#[galeon_engine::command]`, `query`, `event`,
  `dto` — each derives serde, implements marker trait + `ProtocolMeta`. Compile-
  fail tests for invalid usage (enums, tuple structs)
  ([#46](https://github.com/galeon-engine/galeon/issues/46))
- Protocol marker traits: `Command`, `Query`, `Event`, `Dto` with serde +
  `Send + Sync + 'static` bounds; `ProtocolMeta` metadata trait; `ProtocolKind`
  enum. Re-exported from `galeon_engine::protocol`
  ([#45](https://github.com/galeon-engine/galeon/issues/45))
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
