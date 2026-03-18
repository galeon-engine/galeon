# Changelog

All notable changes to the Galeon Engine are documented here.

Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Versioning follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

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
