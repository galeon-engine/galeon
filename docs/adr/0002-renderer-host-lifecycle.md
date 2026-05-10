# ADR-0002: Renderer Host Lifecycle Boundary

**Status:** Accepted
**Date:** 2026-05-10
**Issue:** [#252](https://github.com/galeon-engine/galeon/issues/252)

## Context

Galeon consumers need to combine authoritative frame streams, Three.js
renderers, UI shells, debug tools, and optional framework adapters. If a UI
component owns the renderer directly, normal UI state changes can recreate the
canvas, scene, animation loop, or renderer resources.

That coupling makes rendering correctness depend on application glue instead
of a reusable engine boundary.

## Decision

### Renderer host owns renderer and canvas lifecycle

`RendererHost` owns the renderer adapter, canvas identity, frame loop,
attachment, sizing, rendering, and disposal. UI shells can attach the canvas
and subscribe to frame callbacks, but toggling unrelated UI should not recreate
the host.

### Backend adapter stays host-side

The host depends on a small adapter interface instead of hard-coding a specific
Three.js renderer class. WebGL and WebGPU renderer construction remains an
adapter concern, coordinated with #153.

### Frame ingestion remains separate from scene reconciliation

`TransformFrameIngestion` buffers authoritative `FramePacket` transform state
for render-time sampling. It does not own simulation ticking and does not
replace `RendererCache`; it gives hosts and camera controllers a stable
interpolation boundary between authority updates and render frames.

### Errors must be surfaced

Frame-loop callback and render errors are part of the host lifecycle contract.
They must be reported through host error handling instead of silently freezing
the canvas.

## Consequences

- Renderer identity can remain stable across UI state changes.
- Games can keep Rust as the authority while smoothing render-time samples on
  the host side.
- Devtools and UI shells can attach to a host without owning its resources.
- Resource catalogs, camera controllers, and runtime verification can build on
  a stable lifecycle boundary in follow-up tasks.
