// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

//! One-shot event bridge for audio/VFX triggers across the render boundary.
//!
//! Games implement [`RenderEvent`] on their ECS event types to declare how
//! each event serialises into a [`FrameEvent`]. They then register those
//! event types with [`RenderEventRegistry`]. The extraction pass drains all
//! readable events (from the previous tick) and appends them to the
//! `FramePacket::events` buffer.
//!
//! # Example
//!
//! ```rust
//! # use galeon_engine::render_event::{FrameEvent, RenderEvent};
//! struct ImpactEvent {
//!     entity_index: u32,
//!     position: [f32; 3],
//!     force: f32,
//! }
//!
//! impl RenderEvent for ImpactEvent {
//!     const KIND: u32 = 1;
//!     fn entity(&self) -> u32 { self.entity_index }
//!     fn position(&self) -> [f32; 3] { self.position }
//!     fn intensity(&self) -> f32 { self.force }
//!     // data() defaults to [0.0; 4] — override for extra payload
//! }
//! ```

use crate::event::Events;
use crate::world::World;

// =============================================================================
// FrameEvent — fixed-schema one-shot event for audio/VFX triggers
// =============================================================================

/// A single one-shot event extracted from the ECS for TS consumption.
///
/// Fixed schema optimised for audio/VFX triggers: event type, source entity,
/// world-space position (for spatial audio), intensity (for volume/scale),
/// and a 4-float payload for arbitrary extra data (color, direction, variant).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameEvent {
    /// Stable event-type identifier. Each distinct event type returns a unique
    /// constant via [`RenderEvent::KIND`].
    pub kind: u32,
    /// Source entity index, or 0 if not entity-specific.
    pub entity: u32,
    /// World-space position for spatial audio. `[0.0, 0.0, 0.0]` if not spatial.
    pub position: [f32; 3],
    /// Intensity/magnitude for volume or scale. `1.0` is the default.
    pub intensity: f32,
    /// Extra payload for event-specific data (color, direction, variant ID, etc.).
    /// `[0.0; 4]` when unused.
    pub data: [f32; 4],
}

/// Number of f32-equivalent values per [`FrameEvent`] in the struct-of-arrays
/// WASM transport (kinds + entities + positions + intensities + data).
pub const FRAME_EVENT_STRIDE: usize = 10;

// =============================================================================
// RenderEvent trait
// =============================================================================

/// Implemented by ECS event types that should be extracted as [`FrameEvent`]s
/// for the TS audio/VFX layer.
///
/// Each implementor must provide a unique [`KIND`](RenderEvent::KIND) constant
/// and methods to extract entity, position, and intensity.
pub trait RenderEvent: 'static + Send + Sync {
    /// Unique event-type identifier. Must be stable across frames.
    const KIND: u32;

    /// Source entity index, or 0 if not entity-specific.
    fn entity(&self) -> u32;

    /// World-space position for spatial audio.
    fn position(&self) -> [f32; 3];

    /// Intensity/magnitude for volume scaling. Defaults to `1.0`.
    fn intensity(&self) -> f32 {
        1.0
    }

    /// Extra payload for event-specific data (e.g. color `[r,g,b,a]`,
    /// direction `[dx,dy,dz,0]`, variant ID `[id,0,0,0]`).
    /// Defaults to `[0.0; 4]`.
    fn data(&self) -> [f32; 4] {
        [0.0; 4]
    }
}

// =============================================================================
// RenderEventRegistry
// =============================================================================

/// Type-erased extraction closure that drains readable events from the world.
type EventExtractFn = Box<dyn Fn(&World) -> Vec<FrameEvent> + Send + Sync>;

/// Resource that holds all registered render-event extractors.
///
/// Register event types at startup (inside a [`Plugin`](crate::engine::Plugin)):
///
/// ```rust,no_run
/// # use galeon_engine::render_event::RenderEventRegistry;
/// let mut registry = RenderEventRegistry::new();
/// // registry.register::<ImpactEvent>();
/// ```
pub struct RenderEventRegistry {
    extractors: Vec<EventExtractFn>,
}

impl RenderEventRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            extractors: Vec::new(),
        }
    }

    /// Register an ECS event type for render extraction.
    ///
    /// During frame extraction, all events of type `T` — both from the
    /// previous tick (`read()`) and the current tick (`read_current()`) —
    /// are converted to [`FrameEvent`]s via the [`RenderEvent`] trait.
    /// Reading both buffers eliminates the extra tick of latency that
    /// reading only `previous` would impose, since extraction runs after
    /// the tick completes.
    pub fn register<T: RenderEvent>(&mut self) {
        self.extractors.push(Box::new(|world: &World| {
            let Some(events) = world.try_resource::<Events<T>>() else {
                return Vec::new();
            };
            let to_frame_event = |e: &T| FrameEvent {
                kind: T::KIND,
                entity: e.entity(),
                position: e.position(),
                intensity: e.intensity(),
                data: e.data(),
            };
            events
                .read()
                .chain(events.read_current())
                .map(to_frame_event)
                .collect()
        }));
    }

    /// Drain all readable events from all registered types into a flat buffer.
    pub fn extract_all(&self, world: &World) -> Vec<FrameEvent> {
        let mut result = Vec::new();
        for extractor in &self.extractors {
            result.extend(extractor(world));
        }
        result
    }

    /// Number of registered event types.
    pub fn len(&self) -> usize {
        self.extractors.len()
    }

    /// Returns `true` when no event types are registered.
    pub fn is_empty(&self) -> bool {
        self.extractors.is_empty()
    }
}

impl Default for RenderEventRegistry {
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
    use crate::world::World;

    // -- Test event types ----------------------------------------------------

    #[derive(Debug, PartialEq)]
    struct ImpactEvent {
        entity_index: u32,
        pos: [f32; 3],
        force: f32,
    }

    impl RenderEvent for ImpactEvent {
        const KIND: u32 = 1;
        fn entity(&self) -> u32 {
            self.entity_index
        }
        fn position(&self) -> [f32; 3] {
            self.pos
        }
        fn intensity(&self) -> f32 {
            self.force
        }
    }

    #[derive(Debug, PartialEq)]
    struct ExplosionEvent {
        pos: [f32; 3],
        radius: f32,
    }

    impl RenderEvent for ExplosionEvent {
        const KIND: u32 = 2;
        fn entity(&self) -> u32 {
            0
        }
        fn position(&self) -> [f32; 3] {
            self.pos
        }
        fn intensity(&self) -> f32 {
            self.radius
        }
    }

    // -- Tests ---------------------------------------------------------------

    #[test]
    fn empty_registry() {
        let registry = RenderEventRegistry::new();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
    }

    #[test]
    fn register_increments_len() {
        let mut registry = RenderEventRegistry::new();
        registry.register::<ImpactEvent>();
        assert_eq!(registry.len(), 1);
        registry.register::<ExplosionEvent>();
        assert_eq!(registry.len(), 2);
    }

    #[test]
    fn extract_empty_when_no_events() {
        let mut world = World::new();
        world.add_event::<ImpactEvent>();

        let mut registry = RenderEventRegistry::new();
        registry.register::<ImpactEvent>();

        let events = registry.extract_all(&world);
        assert!(events.is_empty());
    }

    #[test]
    fn extract_empty_when_events_resource_missing() {
        let world = World::new();

        let mut registry = RenderEventRegistry::new();
        registry.register::<ImpactEvent>();

        let events = registry.extract_all(&world);
        assert!(events.is_empty());
    }

    #[test]
    fn extract_events_from_previous_tick() {
        let mut world = World::new();
        world.add_event::<ImpactEvent>();

        world
            .resource_mut::<Events<ImpactEvent>>()
            .send(ImpactEvent {
                entity_index: 42,
                pos: [1.0, 2.0, 3.0],
                force: 0.75,
            });

        // Advance: current → previous, making events readable.
        world.update_events();

        let mut registry = RenderEventRegistry::new();
        registry.register::<ImpactEvent>();

        let events = registry.extract_all(&world);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, 1);
        assert_eq!(events[0].entity, 42);
        assert_eq!(events[0].position, [1.0, 2.0, 3.0]);
        assert!((events[0].intensity - 0.75).abs() < f32::EPSILON);
        assert_eq!(events[0].data, [0.0; 4]);
    }

    #[test]
    fn extract_multiple_events_same_type() {
        let mut world = World::new();
        world.add_event::<ImpactEvent>();

        let events_res = world.resource_mut::<Events<ImpactEvent>>();
        events_res.send(ImpactEvent {
            entity_index: 1,
            pos: [0.0; 3],
            force: 1.0,
        });
        events_res.send(ImpactEvent {
            entity_index: 2,
            pos: [5.0; 3],
            force: 0.5,
        });
        world.update_events();

        let mut registry = RenderEventRegistry::new();
        registry.register::<ImpactEvent>();

        let events = registry.extract_all(&world);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].entity, 1);
        assert_eq!(events[1].entity, 2);
    }

    #[test]
    fn extract_multiple_event_types() {
        let mut world = World::new();
        world.add_event::<ImpactEvent>();
        world.add_event::<ExplosionEvent>();

        world
            .resource_mut::<Events<ImpactEvent>>()
            .send(ImpactEvent {
                entity_index: 10,
                pos: [1.0, 0.0, 0.0],
                force: 0.9,
            });
        world
            .resource_mut::<Events<ExplosionEvent>>()
            .send(ExplosionEvent {
                pos: [5.0, 5.0, 5.0],
                radius: 3.0,
            });
        world.update_events();

        let mut registry = RenderEventRegistry::new();
        registry.register::<ImpactEvent>();
        registry.register::<ExplosionEvent>();

        let events = registry.extract_all(&world);
        assert_eq!(events.len(), 2);

        let impact = events.iter().find(|e| e.kind == 1).unwrap();
        assert_eq!(impact.entity, 10);

        let explosion = events.iter().find(|e| e.kind == 2).unwrap();
        assert_eq!(explosion.entity, 0);
        assert!((explosion.intensity - 3.0).abs() < f32::EPSILON);
    }

    #[test]
    fn current_buffer_events_readable_without_update() {
        let mut world = World::new();
        world.add_event::<ImpactEvent>();

        world
            .resource_mut::<Events<ImpactEvent>>()
            .send(ImpactEvent {
                entity_index: 1,
                pos: [0.0; 3],
                force: 1.0,
            });

        // No update_events() — events are in `current`. Extraction reads
        // both buffers to eliminate the extra-tick latency penalty.
        let mut registry = RenderEventRegistry::new();
        registry.register::<ImpactEvent>();

        let events = registry.extract_all(&world);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].entity, 1);
    }

    #[test]
    fn both_buffers_extracted_no_duplicates_after_update() {
        let mut world = World::new();
        world.add_event::<ImpactEvent>();

        // Tick N: send event A.
        world
            .resource_mut::<Events<ImpactEvent>>()
            .send(ImpactEvent {
                entity_index: 1,
                pos: [0.0; 3],
                force: 1.0,
            });
        // Swap: event A moves to `previous`.
        world.update_events();

        // Tick N+1: send event B (goes to `current`).
        world
            .resource_mut::<Events<ImpactEvent>>()
            .send(ImpactEvent {
                entity_index: 2,
                pos: [5.0; 3],
                force: 0.5,
            });

        // Extract reads both buffers: A from previous, B from current.
        let mut registry = RenderEventRegistry::new();
        registry.register::<ImpactEvent>();

        let events = registry.extract_all(&world);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].entity, 1);
        assert_eq!(events[1].entity, 2);
    }

    #[test]
    fn default_intensity_is_one() {
        #[derive(Debug)]
        struct MinimalEvent;

        impl RenderEvent for MinimalEvent {
            const KIND: u32 = 99;
            fn entity(&self) -> u32 {
                0
            }
            fn position(&self) -> [f32; 3] {
                [0.0; 3]
            }
            // intensity() uses the default impl → 1.0
        }

        let mut world = World::new();
        world.add_event::<MinimalEvent>();
        world
            .resource_mut::<Events<MinimalEvent>>()
            .send(MinimalEvent);
        world.update_events();

        let mut registry = RenderEventRegistry::new();
        registry.register::<MinimalEvent>();

        let events = registry.extract_all(&world);
        assert_eq!(events.len(), 1);
        assert!((events[0].intensity - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn custom_data_payload() {
        #[derive(Debug)]
        struct HitFlash {
            color: [f32; 4],
        }

        impl RenderEvent for HitFlash {
            const KIND: u32 = 50;
            fn entity(&self) -> u32 {
                0
            }
            fn position(&self) -> [f32; 3] {
                [0.0; 3]
            }
            fn data(&self) -> [f32; 4] {
                self.color
            }
        }

        let mut world = World::new();
        world.add_event::<HitFlash>();
        world.resource_mut::<Events<HitFlash>>().send(HitFlash {
            color: [1.0, 0.0, 0.0, 0.8],
        });
        world.update_events();

        let mut registry = RenderEventRegistry::new();
        registry.register::<HitFlash>();

        let events = registry.extract_all(&world);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, 50);
        assert_eq!(events[0].data, [1.0, 0.0, 0.0, 0.8]);
    }

    #[test]
    fn extract_all_with_empty_registry_returns_empty() {
        let world = World::new();
        let registry = RenderEventRegistry::new();
        let events = registry.extract_all(&world);
        assert!(events.is_empty());
    }
}
