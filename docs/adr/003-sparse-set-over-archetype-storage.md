# ADR-003: Sparse Set Storage Over Archetype Storage

**Status:** accepted
**Date:** 2026-03-18
**Issue:** #3

## Context

ECS component storage has two main approaches:

1. **Archetype-based** (Bevy, flecs) — entities with the same component set
   share a table. Adding/removing components moves the entity between tables.
   Excellent iteration speed for large entity counts.

2. **Sparse set** (EnTT) — each component type has its own sparse array indexed
   by entity ID. O(1) insert/get/remove. Dense array gives good iteration.
   Adding/removing components is a cheap swap-remove, no table migration.

The target game (Abyssal Titans) has ~1000 entities in a typical match. RTS
units frequently gain/lose components: depth transition buffs, combat status
effects, research unlocks, formation assignments.

## Decision

**Sparse set storage.** Each component type backed by a sparse set with O(1)
operations and dense iteration.

Type-erased via `Box<dyn Any>` for simplicity. The downcast cost is negligible
at our entity counts.

## Consequences

- **Easier:** Simple implementation (~200 lines). Fast add/remove — ideal for
  RTS entities that frequently change component sets. No archetype migration
  overhead.
- **Harder:** Iteration is slower than archetype tables for very large entity
  counts (>10K). Type erasure via `Box<dyn Any>` adds indirection. Multi-component
  queries require probing multiple sparse sets.

If profiling shows iteration as a bottleneck at higher entity counts, migrate to
archetype storage. The World API (`spawn`/`query`) would stay the same — only the
internal storage changes.
