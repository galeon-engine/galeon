// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

/// Marker trait for types that can be stored as ECS components.
///
/// Derive with `#[derive(Component)]` from `galeon_engine_macros`.
pub trait Component: Send + Sync + 'static {}
