# ADR-005: Rust-First — TypeScript Only Where Unavoidable

**Status:** accepted
**Date:** 2026-03-18

## Context

The engine was initially framed as "Rust + TypeScript" with TS handling runtime
bindings, schedule runner, and editor shell. But the actual goal is closer to
**Bevy with a Three.js renderer** — a Rust-native game engine that happens to
render via Three.js in the browser.

TypeScript adds cognitive overhead, a second type system, a second build
pipeline, and serialization boundaries at every Rust↔JS crossing. Every line
of TS is a line that isn't benefiting from Rust's type safety, determinism,
and WASM portability.

## Decision

**Rust-first. TypeScript only where browser APIs force it.**

TS is acceptable for:
- Three.js renderer integration (Three.js is a JS library — no choice)
- DOM/CSS for the editor shell UI (Solid.js panels around the viewport)
- Browser-specific APIs (WebRTC, WebAudio, pointer events) that have no
  Rust/WASM equivalent

Everything else is Rust:
- ECS, scheduler, plugin system
- Game logic, AI, networking protocol
- Data loading, serialization, asset pipeline
- State management, event bus
- Schedule runner (was planned as TS — should be Rust, called from WASM)

The TS layer is a **thin bridge**: it receives serialized snapshots from
Rust/WASM and translates them into Three.js scene graph updates. It does
not own game state, run game logic, or make gameplay decisions.

## Consequences

- **Easier:** Single language for all engine logic. Deterministic everywhere.
  Smaller TS surface = fewer serialization boundaries. Closer to Bevy's
  architecture (familiar to Rust gamedev community).
- **Harder:** More Rust/WASM code to compile. `wasm-bindgen` bindings for
  every JS API we need. Editor shell panels need Rust→JS data flow for
  reactive UI. Some browser APIs are awkward from WASM.

The `@galeon/engine-ts` package shrinks from "schedule runner" to "WASM
bridge + Three.js sync". `@galeon/runtime` may be absorbed into the WASM
bridge entirely. `@galeon/shell` stays TS (Solid.js UI is inherently DOM).
