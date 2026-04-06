# Changelog

All notable changes to the Galeon Engine are documented here.

Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Versioning follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **`#[handler]` registration + validation (#162)** — New `#[handler]` attribute
  macro registers handler metadata (function name, module path, request/response/error
  types) via `inventory` for downstream code generation. Validates that targets are
  public, synchronous, have a request parameter, and return `Result<R, E>`.
  Compile-fail tests cover async fn, private fn, missing params, and wrong return type.

### Fixed

- **Shiplog label drift (#103)** — Audited all open issues and backfilled
  lifecycle labels (`shiplog/ready`, `shiplog/in-progress`) to match envelope
  `readiness` fields. Added `docs/guide/shiplog-labels.md` with the label
  taxonomy, audit query, and drift prevention rule.

## [0.2.0]

### Added

- **Audio/VFX event bridge (`RenderEvent` + `FrameEvent`)** — One-shot ECS events
  can now flow to the TypeScript layer for triggering audio and visual effects.
  Games implement `RenderEvent` on their event types and register them with
  `RenderEventRegistry`. Events are extracted alongside transforms into
  `FramePacket::events` as fixed-schema `FrameEvent` structs (kind, entity,
  position, intensity, data). Each event carries a 4-float `data` payload for
  arbitrary extra parameters (color, direction, variant ID). The WASM bridge
  exposes struct-of-arrays getters (`event_kinds`, `event_entities`,
  `event_positions`, `event_intensities`, `event_data`).
  Both full and incremental extraction paths include events.
  ([#86](https://github.com/galeon-engine/galeon/issues/86))

- **Entity hierarchy (`ParentEntity` component)** — `ParentEntity(Entity)` attaches
  a child entity to a parent in the render scene graph. FramePacket carries
  `parent_ids` (parallel array, `SCENE_ROOT` sentinel). Extraction depth-sorts
  entities so parents precede children. RendererCache applies a two-pass strategy:
  create/update objects, then reparent via Three.js `add`/`remove`. Orphaned
  children are reparented to the scene root on parent removal.
  ([#135](https://github.com/galeon-engine/galeon/issues/135))

- **Demand rendering — skip `applyFrame()` when nothing changed** —
  `FramePacket` now carries a `frame_version` (sourced from `World::change_tick()`).
  `RendererCache` early-outs when the version is unchanged and exposes a `needsRender`
  getter so game loops can also skip `renderer.render()`. Backward-compatible: packets
  without `frame_version` always process.
  ([#137](https://github.com/galeon-engine/galeon/issues/137))

- **`ObjectType` component and Object3D type diversity in RendererCache** —
  Entities can now specify their Three.js representation via an `ObjectType`
  component (Mesh, PointLight, DirectionalLight, LineSegments, Group). The
  RendererCache factory creates the correct object type, skipping geometry/material
  resolution for types that don't need them. Backward-compatible: entities without
  `ObjectType` default to Mesh.
  ([#134](https://github.com/galeon-engine/galeon/issues/134))

- **`RendererCache.onEntityRemoved` callback** — Notifies consumers when an entity
  is removed (despawn, stale-generation eviction, or `clear()`), allowing explicit
  disposal of consumer-owned GPU resources. The cache no longer auto-disposes
  consumer-provided geometry or materials — ownership is explicit, not inferred.
  ([#131](https://github.com/galeon-engine/galeon/issues/131))

- **`WasmFramePacket.change_flags` and `RendererCache` incremental gating** —
  WASM exposes per-entity change bitmasks; `@galeon/engine-ts` applies transform,
  visibility, and mesh/material updates only when the corresponding flags are set
  (full frames omit flags and behave as before).
  ([#132](https://github.com/galeon-engine/galeon/issues/132))

- **Public package matrix, versioning policy, and stability docs** &mdash; README now
  documents all published crates/packages, the pre-1.0 versioning policy, and
  consumer stability expectations. Publishing guide cross-references the README.
  ([#138](https://github.com/galeon-engine/galeon/issues/138),
  [#139](https://github.com/galeon-engine/galeon/issues/139),
  [#141](https://github.com/galeon-engine/galeon/issues/141))

### Fixed

- **RendererCache regression in #149** — Restored `GALEON_ENTITY_KEY`, per-mesh
  `userData` back-pointer stamping, `matrixAutoUpdate = false`, and
  `updateMatrix()` after transform writes (required when auto-update is off).
  The first #149 diff had dropped these relative to `master`.
  ([#149](https://github.com/galeon-engine/galeon/pull/149))

- **CLI scaffold uses published crate** — `galeon new` templates now reference
  `galeon-engine = "0.1.1"` (crates.io) instead of a git dependency, so generated
  projects resolve against the published release rather than the live `master` branch.
  ([#140](https://github.com/galeon-engine/galeon/issues/140))

## [0.1.1]

### Added

- **Version bump script** — `bash scripts/bump-version.sh X.Y.Z` updates all 6
  lockstep version sources (7 edits). Validates SemVer 2.0.0, checks current
  versions are consistent, and rolls back if verification fails. Supports
  prerelease and build metadata tags.
  ([#128](https://github.com/galeon-engine/galeon/issues/128))

### Changed

- **GitHub Release automation** — tag-triggered releases now create/update a
  GitHub Release after publish + verification succeed, attach the evidence
  markdown as a release asset, and mark prerelease tags as prereleases.
  Verify-only workflow dispatches continue to skip release creation.
  ([#101](https://github.com/galeon-engine/galeon/issues/101))
- **Tag-triggered release workflow** — `release.yml` now triggers on `v*` tag pushes
  instead of manual `workflow_dispatch`. CI runs as a gate via `workflow_call` before
  any publish step. Crates.io propagation uses `cargo search` polling (30 x 10s)
  instead of `sleep 45`. npm publish guards skip already-published versions.
  Prerelease tags (`v0.2.0-alpha.1`) map to the correct npm dist-tag (`alpha`, `beta`,
  `rc`). Post-publish verification installs from registries. Evidence bundle uploaded
  as workflow artifact. `workflow_dispatch` retained as verify-only escape hatch.
  ([#126](https://github.com/galeon-engine/galeon/issues/126))
- **Workspace version inheritance** — Publishable crate versions now inherit from
  `[workspace.package] version` in the root `Cargo.toml` instead of each crate
  declaring its own `version`. Publishing guide updated with explicit version bump
  checklist listing all pin locations.
  ([#126](https://github.com/galeon-engine/galeon/issues/126))

### Fixed

- **Shell scripts stay LF-encoded in Git checkouts** — `.gitattributes` now
  forces `*.sh` to `eol=lf`, keeping Bash-based release tooling runnable in
  fresh Windows worktrees with `core.autocrlf` enabled.
- **RendererCache no longer stomps consumer material/geometry overrides** —
  `applyFrame()` now compares handle IDs (integers) instead of resolved Three.js
  object references. Consumers can safely override `obj.material` or `obj.geometry`
  (e.g. multi-material arrays for per-face texturing) without the cache resetting
  them every frame. Missing registry handles now emit a one-shot `console.warn`
  per entity instead of silently falling back to the magenta wireframe placeholder.
  ([#124](https://github.com/galeon-engine/galeon/issues/124))

### Added

- **npm publishing surface** — `@galeon/runtime`, `@galeon/engine-ts`, and `@galeon/shell`
  now emit JS + declarations to `dist/`, include proper `exports`/`types`/`main` fields,
  and are publishable to npm. `workspace:*` replaced with exact version pins (`=0.1.0`).
  CI validates `npm pack --dry-run` on every PR. Release workflow supports npm with
  OIDC trusted publishing (provenance). Publishing guide updated for both registries.
  ([#122](https://github.com/galeon-engine/galeon/issues/122))
- **Crates.io publishing surface** — crate metadata (`description`, `keywords`, `categories`),
  `publish = false` on non-registry crates (`galeon-cli`, test crates), and pinned
  `version = "=0.1.0"` on path dependencies between publishable crates. CI dry-run
  validation for `galeon-engine-macros`. Release workflow and publishing guide added.
  ([#112](https://github.com/galeon-engine/galeon/issues/112))
- **Dynamic entity spawn/despawn from JS** — `WasmEngine::spawn_entity(mesh_id, material_id, transform)`
  creates a renderable entity at runtime and returns `[index, generation]`.
  `WasmEngine::despawn_entity(index, generation)` removes it. Both take effect on the
  next `extract_frame()` call. `Entity::from_raw(index, generation)` public constructor
  enables round-tripping entity IDs across the WASM boundary. A `JsSpawned` marker
  component guards ownership: `despawn_entity` rejects plugin-spawned entities (returns
  `false`), and `despawn_all_js_entities` provides bulk cleanup. `js_entity_count`
  reports the current JS-spawned entity count for leak detection.
  ([#120](https://github.com/galeon-engine/galeon/issues/120))
- **Consumer-owned WASM bootstrap seam** — `WasmEngine::from_engine(...)`,
  `WasmEngine::engine()`, and `WasmEngine::engine_mut()` let app-owned wrapper
  crates seed plugins, resources, and entities before the first extracted
  frame, without patching the generic bridge crate.
  ([#109](https://github.com/galeon-engine/galeon/issues/109))
- **Configurable tick rate with genre presets** — `Engine::set_tick_rate(hz)` builder method
  sets the fixed-timestep rate. Genre presets: `FixedTimestep::default_rts()` (10 Hz),
  `::strategy()` (20 Hz), `::action()` (30 Hz), `::fast()` (60 Hz). Defaults to 10 Hz if
  not configured.
  ([#98](https://github.com/galeon-engine/galeon/issues/98))
- **`Mut<T>` smart pointer for lazy change-tick stamping** — mutable queries now yield `Mut<T>`
  instead of `&mut T`. Reading via `Deref` does not stamp `changed_tick`; only writing via
  `DerefMut` does. `query_changed` and `extract_frame_incremental` now see only entities that
  were actually mutated. `Mut::set_changed()` is available for components using interior
  mutability (atomics, locks).
  ([#92](https://github.com/galeon-engine/galeon/issues/92))
- **Galeon CLI (`galeon new`)** — binary crate `galeon-cli` provides `galeon new <project> --preset <preset>`
  to scaffold a complete Galeon game project with protocol, domain, server, and db crates.
  Three presets: `server-authoritative`, `local-first`, `hybrid`.
  ([#71](https://github.com/galeon-engine/galeon/issues/71))
- **Per-surface TypeScript codegen** — `generate_typescript_for_surface(&manifest, surface)` emits
  a self-contained TypeScript module containing only the protocol items belonging to that surface.
  `generate_all_surface_typescripts(&manifest)` returns one module per surface. Single-surface
  projects get identical output to the existing `generate_typescript()`.
  ([#81](https://github.com/galeon-engine/galeon/issues/81))
- **Protocol surface metadata** — protocol attribute macros now accept `surface = "..."` and
  `surfaces = ["...", "..."]`, `ProtocolManifest::collect_with_default_surface(...)` can rename
  the implicit default surface, and manifest entries record explicit surface memberships for
  multi-API projects.
  ([#82](https://github.com/galeon-engine/galeon/issues/82))
- **Deadline scheduler** — UTC-based timed event firing. `Timestamp` (microseconds since epoch),
  `Clock` trait with `SystemClock` and `TestClock`, `Deadlines<T>` sorted resource, `DeadlineId`
  for cancellation. Integrates with Events API — fired deadlines become `Events<T>` readable via
  `EventReader<T>`. Batch reconciliation fires all overdue in one tick. Commands integration via
  `Commands::schedule_deadline()` and `Commands::cancel_deadline()`.
  ([#79](https://github.com/galeon-engine/galeon/issues/79))
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

- **BREAKING: Mutable query item type changed from `&mut T` to `Mut<T>`** — `QueryIterMut`,
  `World::query_mut`, `World::one_mut`, and `World::get_mut` now return `Mut<T>` wrappers.
  Direct callers need `mut` bindings (e.g., `for (_, mut pos) in world.query_mut::<&mut Pos>()`).
  `QueryMut` system parameter (`fn(QueryMut<T>)`) is transparent — `&mut Mut<T>` auto-derefs.
  ([#92](https://github.com/galeon-engine/galeon/issues/92))
- **BREAKING: Protocol manifest/descriptors now carry surface grouping** — manifest schema version
  is now `2`, manifests expose `default_surface` plus resolved `surfaces`, and
  `generate_descriptors(&manifest)` returns per-surface descriptor groups instead of one flat list.
  Single-surface projects still work without annotations; multi-surface projects now keep shared
  items explicit instead of flattening everything into one generated surface.
  ([#82](https://github.com/galeon-engine/galeon/issues/82))
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
