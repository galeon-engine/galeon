# ADR-004: Cargo + Bun Dual Workspace Layout

**Status:** accepted
**Date:** 2026-03-18
**Issue:** #1

## Context

The engine spans two languages: Rust (ECS, simulation, WASM bridge) and
TypeScript (runtime bindings, schedule runner, editor shell). Each needs its
own package manager and workspace tooling.

Options considered:
- **Nx** for TS — rejected as overkill for 3 packages
- **Turborepo** for TS — rejected, same reason
- **Bun workspaces** — native, zero extra tooling
- **Single flat Rust crate** — rejected because proc-macros require a separate
  crate and `cdylib` (WASM) can't coexist with `proc-macro`

## Decision

- **Rust:** Cargo workspace at root with `crates/*` members. Resolver 2,
  edition 2024. Shared dependency versions in `[workspace.dependencies]`.
- **TypeScript:** Bun workspaces with `packages/*`. Project references via
  `tsconfig.json` for `tsc --build`. Paths mapping in `tsconfig.base.json`
  for cross-package imports (Bun on Windows doesn't create workspace symlinks).

Three Rust crates: `engine-macros` → `engine` → `engine-three-sync`.
Three TS packages: `@galeon/runtime` → `@galeon/engine-ts`, `@galeon/shell`.

## Consequences

- **Easier:** Each crate/package compiles independently. CI checks both
  ecosystems in parallel. Adding new crates is just `cargo init`.
- **Harder:** Two lockfiles (`Cargo.lock`, `bun.lock`). Cross-language
  dependency (WASM bridge) requires manual coordination. Windows Bun
  workspace symlinks don't work — needed `paths` workaround in tsconfig.
