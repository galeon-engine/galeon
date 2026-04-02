// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

//! Custom per-entity data channels for the render extraction pipeline.
//!
//! Games implement [`ExtractToFloats`] on their components to declare how
//! component state serialises into a flat `f32` buffer. They then register
//! those components by name with [`RenderChannelRegistry`]. The extraction
//! pass iterates every registered channel for every visible entity and
//! populates the corresponding float slice in `FramePacket`.

use crate::component::Component;
use crate::entity::Entity;
use crate::world::World;

/// Type alias for the type-erased per-entity extraction closure stored in a
/// [`ChannelRegistration`].
///
/// The closure receives a `&World`, the target `Entity`, and a mutable slice
/// of exactly `stride` floats. It returns `true` when the entity has the
/// component and `false` (with a zeroed buffer) when it is absent.
pub type ExtractFn = Box<dyn Fn(&World, Entity, &mut [f32]) -> bool + Send + Sync>;

// =============================================================================
// ExtractToFloats trait
// =============================================================================

/// Implemented by components that want to contribute per-entity float data to
/// the renderer.
///
/// # Example
///
/// ```rust
/// # use galeon_engine::component::Component;
/// # use galeon_engine::render_channel::ExtractToFloats;
/// struct WearState { wear: f32, heat: f32 }
/// impl Component for WearState {}
/// impl ExtractToFloats for WearState {
///     const STRIDE: usize = 2;
///     fn extract(&self, buf: &mut [f32]) {
///         buf[0] = self.wear;
///         buf[1] = self.heat;
///     }
/// }
/// ```
pub trait ExtractToFloats: Component {
    /// Number of `f32` values produced per entity.
    const STRIDE: usize;

    /// Write component data into `buf`.
    ///
    /// `buf` is always exactly `STRIDE` elements long.
    fn extract(&self, buf: &mut [f32]);
}

// =============================================================================
// ChannelRegistration (type-erased)
// =============================================================================

/// Type-erased entry for a single registered render channel.
// `stride` and `extract_fn` are read by the extraction pass and debug snapshot.
#[allow(dead_code)]
pub struct ChannelRegistration {
    /// Channel name, as supplied by the caller at registration time.
    pub name: String,
    /// Number of `f32` values per entity for this channel.
    pub stride: usize,
    /// Type-erased extraction closure.
    ///
    /// Given a `&World` and an `Entity`, writes `stride` floats into `buf`
    /// (which is always exactly `stride` elements) and returns `true` when the
    /// component is present.  Returns `false` and zeros the buffer when the
    /// entity does not have the component.
    pub extract_fn: ExtractFn,
}

// =============================================================================
// RenderChannelRegistry
// =============================================================================

/// Resource that holds all registered per-entity render channels.
///
/// Register channels at startup (inside a [`Plugin`](crate::engine::Plugin)):
///
/// ```rust,no_run
/// # use galeon_engine::render_channel::{ExtractToFloats, RenderChannelRegistry};
/// # use galeon_engine::component::Component;
/// # use galeon_engine::world::World;
/// # struct WearState;
/// # impl Component for WearState {}
/// # impl ExtractToFloats for WearState {
/// #     const STRIDE: usize = 1;
/// #     fn extract(&self, buf: &mut [f32]) { buf[0] = 0.0; }
/// # }
/// let mut registry = RenderChannelRegistry::new();
/// registry.register::<WearState>("wear");
/// ```
pub struct RenderChannelRegistry {
    pub channels: Vec<ChannelRegistration>,
}

impl RenderChannelRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            channels: Vec::new(),
        }
    }

    /// Register a component type as a named float channel.
    ///
    /// # Panics
    ///
    /// Panics if a channel with the same `name` is already registered.
    pub fn register<T: ExtractToFloats>(&mut self, name: impl Into<String>) {
        let name = name.into();
        if self.channels.iter().any(|c| c.name == name) {
            panic!("render channel '{}' already registered", name);
        }

        let extract_fn =
            Box::new(move |world: &World, entity: Entity, buf: &mut [f32]| {
                match world.get::<T>(entity) {
                    Some(comp) => {
                        comp.extract(buf);
                        true
                    }
                    None => {
                        buf.fill(0.0);
                        false
                    }
                }
            });

        self.channels.push(ChannelRegistration {
            name,
            stride: T::STRIDE,
            extract_fn,
        });
    }

    /// Number of registered channels.
    pub fn len(&self) -> usize {
        self.channels.len()
    }

    /// Returns `true` when no channels are registered.
    pub fn is_empty(&self) -> bool {
        self.channels.is_empty()
    }
}

impl Default for RenderChannelRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::Transform;

    // -------------------------------------------------------------------------
    // Test component
    // -------------------------------------------------------------------------

    #[derive(Debug)]
    struct WearState {
        wear: f32,
        heat: f32,
        tint: [f32; 3],
    }

    impl Component for WearState {}

    impl ExtractToFloats for WearState {
        const STRIDE: usize = 5;

        fn extract(&self, buf: &mut [f32]) {
            buf[0] = self.wear;
            buf[1] = self.heat;
            buf[2] = self.tint[0];
            buf[3] = self.tint[1];
            buf[4] = self.tint[2];
        }
    }

    // -------------------------------------------------------------------------

    #[test]
    fn empty_registry() {
        let registry = RenderChannelRegistry::new();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
    }

    #[test]
    fn register_channel() {
        let mut registry = RenderChannelRegistry::new();
        registry.register::<WearState>("wear");

        assert_eq!(registry.len(), 1);
        assert_eq!(registry.channels[0].name, "wear");
        assert_eq!(registry.channels[0].stride, 5);
    }

    #[test]
    #[should_panic(expected = "already registered")]
    fn duplicate_channel_panics() {
        let mut registry = RenderChannelRegistry::new();
        registry.register::<WearState>("wear");
        registry.register::<WearState>("wear");
    }

    #[test]
    fn extract_fn_returns_data_when_present() {
        let mut world = World::new();
        let entity = world.spawn((WearState {
            wear: 0.75,
            heat: 1.25,
            tint: [0.1, 0.2, 0.3],
        },));

        let mut registry = RenderChannelRegistry::new();
        registry.register::<WearState>("wear");

        let channel = &registry.channels[0];
        let mut buf = vec![0.0f32; channel.stride];
        let found = (channel.extract_fn)(&world, entity, &mut buf);

        assert!(found);
        assert!((buf[0] - 0.75).abs() < f32::EPSILON);
        assert!((buf[1] - 1.25).abs() < f32::EPSILON);
        assert!((buf[2] - 0.1).abs() < f32::EPSILON);
        assert!((buf[3] - 0.2).abs() < f32::EPSILON);
        assert!((buf[4] - 0.3).abs() < f32::EPSILON);
    }

    #[test]
    fn extract_fn_zeroes_when_absent() {
        let mut world = World::new();
        // Spawn with Transform only — no WearState.
        let entity = world.spawn((Transform::identity(),));

        let mut registry = RenderChannelRegistry::new();
        registry.register::<WearState>("wear");

        let channel = &registry.channels[0];
        let mut buf = vec![1.0f32; channel.stride]; // pre-fill with non-zero sentinel
        let found = (channel.extract_fn)(&world, entity, &mut buf);

        assert!(!found);
        assert!(buf.iter().all(|&v| v == 0.0));
    }
}
