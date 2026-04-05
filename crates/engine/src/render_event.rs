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

use std::cell::RefCell;

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

/// Type-erased extraction closure that reads current-tick events from the world.
type EventExtractFn = Box<dyn Fn(&World) -> Vec<FrameEvent> + Send + Sync>;

/// Resource that holds all registered render-event extractors and an
/// accumulation buffer that survives across multiple ticks per frame.
///
/// # Data flow
///
/// ```text
/// Each tick:  Schedule::run() → systems write EventWriter<T>
///                             → world.flush_render_events()
///                               reads Events<T>::current, appends to `pending`
///                             → update_events() swaps buffers (safe — we already captured)
///
/// Each frame: extract_frame() → registry.drain() → moves `pending` into FramePacket
/// ```
///
/// This avoids the double-buffer latency AND prevents multi-tick event loss:
/// every tick's events accumulate until the next extraction drains them.
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
    /// Accumulation buffer: events captured across ticks via [`accumulate`],
    /// drained once per frame by [`drain`]. Uses `RefCell` because drain is
    /// called from `&World` (shared access) during extraction.
    pending: RefCell<Vec<FrameEvent>>,
}

impl RenderEventRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            extractors: Vec::new(),
            pending: RefCell::new(Vec::new()),
        }
    }

    /// Register an ECS event type for render extraction.
    ///
    /// The type-erased extractor reads `Events<T>::current` (this tick's
    /// events, not yet swapped by `update_events`). Called once per tick
    /// via [`accumulate`](Self::accumulate).
    pub fn register<T: RenderEvent>(&mut self) {
        self.extractors.push(Box::new(|world: &World| {
            let Some(events) = world.try_resource::<Events<T>>() else {
                return Vec::new();
            };
            events
                .read_current()
                .map(|e| FrameEvent {
                    kind: T::KIND,
                    entity: e.entity(),
                    position: e.position(),
                    intensity: e.intensity(),
                    data: e.data(),
                })
                .collect()
        }));
    }

    /// Capture this tick's events into the accumulation buffer.
    ///
    /// Called by [`World::flush_render_events`] at the end of each
    /// `Schedule::run()`, **before** `update_events()` swaps the buffers
    /// on the next tick. This ensures every tick's events are preserved
    /// even when multiple ticks run per render frame (same effect as
    /// Bevy's deferred buffer swap, without changing core event semantics).
    pub fn accumulate(&self, world: &World) {
        let mut pending = self.pending.borrow_mut();
        for extractor in &self.extractors {
            pending.extend(extractor(world));
        }
    }

    /// Take all accumulated events since the last drain.
    ///
    /// Called by the extraction pass to move events into the `FramePacket`.
    /// Uses `RefCell` interior mutability so extraction can drain from `&World`.
    pub fn drain(&self) -> Vec<FrameEvent> {
        std::mem::take(&mut *self.pending.borrow_mut())
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

    // Helper: accumulate + drain simulates the schedule → extraction flow.
    fn flush_and_drain(registry: &RenderEventRegistry, world: &World) -> Vec<FrameEvent> {
        registry.accumulate(world);
        registry.drain()
    }

    #[test]
    fn drain_empty_when_no_events() {
        let mut world = World::new();
        world.add_event::<ImpactEvent>();

        let mut registry = RenderEventRegistry::new();
        registry.register::<ImpactEvent>();

        let events = flush_and_drain(&registry, &world);
        assert!(events.is_empty());
    }

    #[test]
    fn drain_empty_when_events_resource_missing() {
        let world = World::new();

        let mut registry = RenderEventRegistry::new();
        registry.register::<ImpactEvent>();

        let events = flush_and_drain(&registry, &world);
        assert!(events.is_empty());
    }

    #[test]
    fn accumulate_captures_current_buffer() {
        let mut world = World::new();
        world.add_event::<ImpactEvent>();

        world
            .resource_mut::<Events<ImpactEvent>>()
            .send(ImpactEvent {
                entity_index: 42,
                pos: [1.0, 2.0, 3.0],
                force: 0.75,
            });

        let mut registry = RenderEventRegistry::new();
        registry.register::<ImpactEvent>();

        let events = flush_and_drain(&registry, &world);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, 1);
        assert_eq!(events[0].entity, 42);
        assert_eq!(events[0].position, [1.0, 2.0, 3.0]);
        assert!((events[0].intensity - 0.75).abs() < f32::EPSILON);
        assert_eq!(events[0].data, [0.0; 4]);
    }

    #[test]
    fn accumulate_multiple_events_same_type() {
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

        let mut registry = RenderEventRegistry::new();
        registry.register::<ImpactEvent>();

        let events = flush_and_drain(&registry, &world);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].entity, 1);
        assert_eq!(events[1].entity, 2);
    }

    #[test]
    fn accumulate_multiple_event_types() {
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

        let mut registry = RenderEventRegistry::new();
        registry.register::<ImpactEvent>();
        registry.register::<ExplosionEvent>();

        let events = flush_and_drain(&registry, &world);
        assert_eq!(events.len(), 2);

        let impact = events.iter().find(|e| e.kind == 1).unwrap();
        assert_eq!(impact.entity, 10);

        let explosion = events.iter().find(|e| e.kind == 2).unwrap();
        assert_eq!(explosion.entity, 0);
        assert!((explosion.intensity - 3.0).abs() < f32::EPSILON);
    }

    #[test]
    fn multi_tick_accumulation_no_event_loss() {
        let mut world = World::new();
        world.add_event::<ImpactEvent>();

        let mut registry = RenderEventRegistry::new();
        registry.register::<ImpactEvent>();

        // Tick 1: send, accumulate, swap.
        world
            .resource_mut::<Events<ImpactEvent>>()
            .send(ImpactEvent {
                entity_index: 1,
                pos: [0.0; 3],
                force: 1.0,
            });
        registry.accumulate(&world);
        world.update_events(); // swap clears current — but we already captured

        // Tick 2: send, accumulate, swap.
        world
            .resource_mut::<Events<ImpactEvent>>()
            .send(ImpactEvent {
                entity_index: 2,
                pos: [5.0; 3],
                force: 0.5,
            });
        registry.accumulate(&world);
        world.update_events();

        // Drain: both ticks' events present, no loss.
        let events = registry.drain();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].entity, 1);
        assert_eq!(events[1].entity, 2);
    }

    #[test]
    fn drain_clears_pending() {
        let mut world = World::new();
        world.add_event::<ImpactEvent>();

        let mut registry = RenderEventRegistry::new();
        registry.register::<ImpactEvent>();

        world
            .resource_mut::<Events<ImpactEvent>>()
            .send(ImpactEvent {
                entity_index: 1,
                pos: [0.0; 3],
                force: 1.0,
            });
        registry.accumulate(&world);

        assert_eq!(registry.drain().len(), 1);
        // Second drain returns empty — no double delivery.
        assert!(registry.drain().is_empty());
    }

    #[test]
    fn no_double_delivery_across_frames() {
        let mut world = World::new();
        world.add_event::<ImpactEvent>();

        let mut registry = RenderEventRegistry::new();
        registry.register::<ImpactEvent>();

        // Frame 1: send, accumulate, drain.
        world
            .resource_mut::<Events<ImpactEvent>>()
            .send(ImpactEvent {
                entity_index: 1,
                pos: [0.0; 3],
                force: 1.0,
            });
        registry.accumulate(&world);
        let frame1 = registry.drain();
        assert_eq!(frame1.len(), 1);

        // Swap (next tick start).
        world.update_events();

        // Frame 2: no new events, accumulate reads empty current.
        registry.accumulate(&world);
        let frame2 = registry.drain();
        assert!(frame2.is_empty()); // NOT re-delivered from previous
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
        }

        let mut world = World::new();
        world.add_event::<MinimalEvent>();
        world
            .resource_mut::<Events<MinimalEvent>>()
            .send(MinimalEvent);

        let mut registry = RenderEventRegistry::new();
        registry.register::<MinimalEvent>();

        let events = flush_and_drain(&registry, &world);
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

        let mut registry = RenderEventRegistry::new();
        registry.register::<HitFlash>();

        let events = flush_and_drain(&registry, &world);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, 50);
        assert_eq!(events[0].data, [1.0, 0.0, 0.0, 0.8]);
    }

    #[test]
    fn drain_with_empty_registry_returns_empty() {
        let registry = RenderEventRegistry::new();
        assert!(registry.drain().is_empty());
    }
}
