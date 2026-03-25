# Changelog

All notable changes to the Galeon Engine are documented here.

Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Versioning follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **ECS Events API** — `Events<T>` double-buffered typed event queue with `EventWriter<T>` and
  `EventReader<T>` system parameters. Events sent in tick N are readable in tick N+1.
  Register with `World::add_event::<T>()`. Auto-cleared by `Schedule::run()`.
  ([#72](https://github.com/galeon-engine/galeon/issues/72))
- **Protocol codegen and handler seam** — `codegen` module generates TypeScript interfaces and
  protocol descriptors from `ProtocolManifest`. `handler` module provides `HandlerRegistry` with
  typed command/query dispatch for both local (in-process) and remote (JSON boundary) adapters.
  ([#69](https://github.com/galeon-engine/galeon/issues/69))
- **Commands API for deferred structural mutations** — `Commands` system parameter buffers
  spawn/despawn/insert/remove operations. Applied automatically between schedule stages via
  `World::apply_commands()`. Avoids mid-iteration archetype changes and enables batching.
  ([#30](https://github.com/galeon-engine/galeon/issues/30))

### Removed

- **BREAKING: Legacy `fn(&mut World)` system path removed** — `LegacySystem`, `LegacySystemFn`,
  `IntoSystem<()> for fn(&mut World)`, `Schedule::add_legacy_system`, and `Engine::add_legacy_system`
  are all gone. Parameterized systems (`fn(Res<T>, QueryMut<U>)`) are now the only supported
  scheduling API. This is intentional pre-release surface reduction — the engine is not public yet.
  ([#65](https://github.com/galeon-engine/galeon/issues/65))

### Changed

- **BREAKING: `protocol::Query` renamed to `protocol::ProtocolQuery`** — frees up the `Query`
  name for the ECS system parameter. Code using `galeon_engine::Query` as the protocol trait must
  update to `galeon_engine::ProtocolQuery`. The `#[galeon::query]` attribute macro is unchanged.
  ([#57](https://github.com/galeon-engine/galeon/issues/57))
- **BREAKING: `QueryParam` / `QueryParamMut` root aliases removed** — `galeon_engine::Query` and
  `galeon_engine::QueryMut` now refer directly to the ECS system parameters (previously
  `system_param::Query` / `system_param::QueryMut`). No alias needed.
  ([#57](https://github.com/galeon-engine/galeon/issues/57))
- **Schedule::run** takes `&mut self` (was `&self`) because `System::run` requires `&mut self`
  ([#33](https://github.com/galeon-engine/galeon/issues/33))
- **Schedule::add_system** now generic over `impl IntoSystem<P>` — accepts parameterized systems like `fn(Res<T>, QueryMut<U>)` (with turbofish for param types)
  ([#33](https://github.com/galeon-engine/galeon/issues/33))
- **Engine::add_system** follows the same generic signature as `Schedule::add_system`
  ([#33](https://github.com/galeon-engine/galeon/issues/33))
- **game_loop::tick** takes `&mut Schedule` (was `&Schedule`)
  ([#33](https://github.com/galeon-engine/galeon/issues/33))
- **World internals**: Replaced sparse-set storage with archetype table storage for cache-friendly iteration and O(1) despawn
  ([#28](https://github.com/galeon-engine/galeon/issues/28))
- **Bundle trait**: Now provides `type_ids()`, `register_columns()`, and `push_into_columns()` for archetype-aware spawning
  ([#28](https://github.com/galeon-engine/galeon/issues/28))
- **query2_mut**: Eliminated `typed_sets_two_mut` unsafe from World — unsafe is now contained in `Archetype::entities_and_two_columns_mut`
  ([#28](https://github.com/galeon-engine/galeon/issues/28))
- **Queries use typed query specs**: `world.query::<&T>()`, `world.query::<(&A, &B)>()`, and `world.query_mut::<(&mut A, &mut B)>()` now return zero-allocation archetype iterators instead of `Vec`
  ([#29](https://github.com/galeon-engine/galeon/issues/29))

### Added

- `World::insert<C: Component>(entity, value)` — add a component to an existing entity (archetype migration)
  ([#28](https://github.com/galeon-engine/galeon/issues/28))
- `World::remove<C: Component>(entity)` — remove a component from an entity (archetype migration)
  ([#28](https://github.com/galeon-engine/galeon/issues/28))
- `ArchetypeStore::get_two_mut` — safe dual mutable archetype access via `split_at_mut`
  ([#28](https://github.com/galeon-engine/galeon/issues/28))
- `Archetype::entities_and_column_mut` — split-borrow for mutable query iteration
  ([#28](https://github.com/galeon-engine/galeon/issues/28))
- `Archetype::entities_and_two_columns_mut` — split-borrow for two-component mutable queries
  ([#28](https://github.com/galeon-engine/galeon/issues/28))
- `Column::iter` / `Column::iter_mut` — dense column iteration
  ([#28](https://github.com/galeon-engine/galeon/issues/28))
- `QuerySpec`, `QuerySpecMut`, and `QueryFilter` traits for typed archetype queries
  ([#29](https://github.com/galeon-engine/galeon/issues/29))
- `World::query_filtered`, `World::query_filtered_mut`, `World::one`, and `World::one_mut`
  ([#29](https://github.com/galeon-engine/galeon/issues/29))
- `With<T>` / `Without<T>` archetype filters
  ([#29](https://github.com/galeon-engine/galeon/issues/29))
- `World::query2`, `query2_mut`, `query3`, and `query3_mut` convenience wrappers plus exact `size_hint` support on archetype query iterators
  ([#32](https://github.com/galeon-engine/galeon/issues/32))
- `SystemParam` trait (unsafe, with GAT `Item<'w>`) — system parameter extraction from `*mut World`
  ([#33](https://github.com/galeon-engine/galeon/issues/33))
- `Res<T>` / `ResMut<T>` — shared/exclusive resource access as system parameters
  ([#33](https://github.com/galeon-engine/galeon/issues/33))
- `Query<T>` / `QueryMut<T>` — shared/exclusive component query as system parameters
  ([#33](https://github.com/galeon-engine/galeon/issues/33))
- `SystemParam` tuple expansion macro (0–8 arity) for multi-parameter systems
  ([#33](https://github.com/galeon-engine/galeon/issues/33))
- `System` trait, `IntoSystem<P>`, `FunctionSystem` — bridge from `fn(Res<A>, Query<B>)` to `System::run`
  ([#33](https://github.com/galeon-engine/galeon/issues/33))
- `Access` enum with intra-system conflict detection — panics at registration if params alias
  ([#33](https://github.com/galeon-engine/galeon/issues/33))

### Removed

- `EntityAllocator` — superseded by `EntityMetaStore` with archetype location tracking
  ([#28](https://github.com/galeon-engine/galeon/issues/28))
- `TypedSparseSet<T>`, `AnyComponentStore`, `ComponentStorage` — superseded by archetype `Column<T>` and `ArchetypeStore`
  ([#28](https://github.com/galeon-engine/galeon/issues/28))
- `QueryIter<'w, T>`, `QueryIterMut<'w, T>`, `Query2Iter`, `Query2MutIter`, `Query3Iter`, `Query3MutIter` — replaced by generic `QueryIter<'w, Q, F>` / `QueryIterMut<'w, Q, F>` (breaking: different generic signatures)
  ([#28](https://github.com/galeon-engine/galeon/issues/28), [#29](https://github.com/galeon-engine/galeon/issues/29))

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
