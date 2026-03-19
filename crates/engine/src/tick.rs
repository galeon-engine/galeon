// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

/// Tick counter for ECS change detection.
///
/// Starts at 1. Tick 0 is reserved as a "before everything" sentinel,
/// so `query_changed(since: 0)` returns all components.
pub type Tick = u64;

/// Sentinel value: "before any tick". Use as `since` argument to get everything.
pub const TICK_ZERO: Tick = 0;
