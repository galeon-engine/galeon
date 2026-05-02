# ADR 0002: Framework-Neutral Render Contract And Host Adapters

## Status

Accepted

## Context

Galeon already extracts render-facing ECS state into a flat `FramePacket` for
WASM and TypeScript consumers. The packet contains entity ids, generations,
transforms, visibility, mesh/material handles, parent ids, object type tags,
custom channels, one-shot events, change flags, and a frame version. That shape
is substantially framework-neutral; the Three.js-specific behavior lives in the
TypeScript `RendererCache` adapter.

React Three Fiber is a useful host for web clients with React-owned UI,
routing, menus, HUDs, and scene composition, but React must not become part of
Galeon's engine core or WASM transport contract.

## Decision

Galeon treats the render packet as the public framework-neutral render snapshot
contract. The contract is versioned with `RENDER_CONTRACT_VERSION` on the Rust
side and `RENDER_CONTRACT_VERSION` in TypeScript. Packets crossing the adapter
boundary expose `contract_version` so host packages can fail fast on
incompatible changes.

Package ownership is split by concern:

- `galeon-engine-three-sync` continues to produce the WASM render packet. The
  crate name remains unchanged for compatibility even though the packet is
  neutral rather than Three-specific.
- `@galeon/render-core` owns TypeScript contract types, constants, and
  validation guardrails.
- `@galeon/three` owns the imperative Three.js scene-cache adapter.
- `@galeon/r3f` owns the React Three Fiber host adapter. React and R3F stay as
  host-level peer dependencies, not core dependencies.

The original split also kept `@galeon/engine-ts` as a one-minor compatibility
re-export of the symbols that moved into `@galeon/render-core` and
`@galeon/three`. That deprecation window closed in `0.5.0` (issue #209) and
the package is no longer published from this repository — see the
"Migrating from `@galeon/engine-ts`" section of `docs/guide/three-sync.md`
for the per-symbol new homes.

The R3F adapter uses React for provider setup, structural entity lifecycle, and
optional hooks over the current entity set. Hot frame updates are applied by
mutating retained Three object refs through the adapter/frame-loop path, not by
forcing every entity transform through React state on each tick.

## Compatibility Policy

Additive packet fields are allowed when older adapters can safely ignore them
or when new adapters can tolerate their absence. Removing fields, changing
array layout, changing discriminant values, changing change-flag semantics, or
changing identity/generation rules requires a contract version bump.

During the first transition, TypeScript validation accepts pre-versioned
packets by default for one-minor compatibility. New strict adapter surfaces may
require `contract_version` immediately.

## Consequences

The engine stays Rust-first and host-framework-neutral while making React Three
Fiber a supported adapter path. The imperative Three adapter remains the
behavioral baseline and compatibility path for existing users. Future render
hosts can integrate against the same contract without importing React or
Three.js adapter code.
