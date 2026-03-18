# ADR-002: Rust ECS as Single Source of Truth

**Status:** accepted
**Date:** 2026-03-18
**Issue:** #3

## Context

The engine needs an authoritative location for all game state. Options:

1. **Rust ECS (WASM-compiled)** — all state in Rust, Three.js reads snapshots
2. **TypeScript ECS** — state in JS, easier to prototype but no determinism
3. **Split state** — some in Rust, some in JS

Deterministic lockstep multiplayer requires bit-identical simulation across
clients. JavaScript floating-point behavior varies across engines. The game
(Abyssal Titans) demands 40+ unit types, 170+ technologies, and 5 depth layers
— all of which must be deterministic.

## Decision

**Rust ECS is the single source of truth.** All game state lives in Rust.
Three.js sync reads serialized snapshots. The ECS compiles to WASM for browser
deployment and runs natively for server/headless modes.

The ECS is owned by the engine (not a third-party library)
to control WASM bundle size and avoid tight coupling to external schedule models.

## Consequences

- **Easier:** Deterministic simulation. Portable to server (native Rust) and
  browser (WASM). Full control over storage model and query API. Minimal
  dependency tree.
- **Harder:** Must implement ECS primitives ourselves. No community ecosystem
  for components/systems. WASM bridge adds serialization overhead for
  Three.js sync.
