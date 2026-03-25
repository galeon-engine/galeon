// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

use std::any::TypeId;

use crate::system_param::{Access, SystemParam};
use crate::world::UnsafeWorldCell;

// =============================================================================
// Events<T> — double-buffered typed event queue
// =============================================================================

/// Double-buffered typed event queue.
///
/// Events are written to `current` during the tick they are sent. At the start
/// of the next `Schedule::run()`, `World::update_events()` swaps the buffers:
/// `current` becomes `previous` and the old `previous` is cleared. Systems
/// that use `EventReader<T>` iterate over `previous`, so they always read
/// events from the *previous* tick.
///
/// ```text
/// Tick N:   EventWriter sends → current
/// tick N+1: update_events() → current becomes previous, cleared current
///           EventReader reads previous (events from tick N)
/// Tick N+2: update_events() → previous is cleared
/// ```
///
/// Register an event type with [`World::add_event::<T>()`] before using
/// `EventWriter<T>` or `EventReader<T>` in systems.
pub struct Events<T: 'static> {
    /// Events written during the previous tick — readable by `EventReader`.
    previous: Vec<T>,
    /// Events being written this tick — by `EventWriter`.
    current: Vec<T>,
}

impl<T: 'static> Events<T> {
    /// Create an empty double buffer.
    pub fn new() -> Self {
        Self {
            previous: Vec::new(),
            current: Vec::new(),
        }
    }

    /// Send an event. It will be readable by `EventReader` on the next tick.
    pub fn send(&mut self, event: T) {
        self.current.push(event);
    }

    /// Iterate over events sent during the previous tick.
    pub fn read(&self) -> impl Iterator<Item = &T> {
        self.previous.iter()
    }

    /// Advance the double buffer.
    ///
    /// The previous buffer is cleared. The current buffer becomes the new
    /// previous buffer. Called automatically by `World::update_events()` at
    /// the start of each `Schedule::run()`.
    pub fn update(&mut self) {
        // Move current → previous (swap), then clear current.
        // Using swap + clear avoids a heap allocation: we reuse the old
        // previous buffer (now cleared) as the new current buffer.
        std::mem::swap(&mut self.previous, &mut self.current);
        self.current.clear();
    }

    /// Number of readable events (in the previous buffer).
    pub fn len(&self) -> usize {
        self.previous.len()
    }

    /// Returns `true` if there are no readable events.
    pub fn is_empty(&self) -> bool {
        self.previous.is_empty()
    }
}

impl<T: 'static> Default for Events<T> {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// EventWriter<'w, T> — exclusive write access as a SystemParam
// =============================================================================

/// Exclusive write access to the `Events<T>` resource.
///
/// Use this in a system to send events that other systems can read on the
/// next schedule tick via [`EventReader<T>`].
///
/// ```rust,ignore
/// fn fire_cannon(mut writer: EventWriter<'_, CannonFired>) {
///     writer.send(CannonFired { power: 9000 });
/// }
/// ```
pub struct EventWriter<'w, T: 'static> {
    events: &'w mut Events<T>,
}

impl<'w, T: 'static> EventWriter<'w, T> {
    /// Send an event. Readable by `EventReader<T>` systems on the next tick.
    pub fn send(&mut self, event: T) {
        self.events.send(event);
    }
}

// SAFETY: access() reports ResWrite(Events<T>). fetch() only touches the
// Events<T> resource field via get_resource_mut — no other field is accessed.
unsafe impl<T: 'static> SystemParam for EventWriter<'_, T> {
    type Item<'w> = EventWriter<'w, T>;

    fn access() -> Vec<Access> {
        vec![Access::ResWrite(TypeId::of::<Events<T>>())]
    }

    unsafe fn fetch<'w>(world: UnsafeWorldCell) -> Self::Item<'w> {
        EventWriter {
            events: unsafe { world.get_resource_mut::<Events<T>>() },
        }
    }
}

// =============================================================================
// EventReader<'w, T> — shared read access as a SystemParam
// =============================================================================

/// Shared read access to the `Events<T>` resource.
///
/// Iterates over events sent by `EventWriter<T>` on the *previous* tick.
///
/// ```rust,ignore
/// fn on_cannon_fired(reader: EventReader<'_, CannonFired>) {
///     for ev in reader.read() {
///         println!("cannon fired with power {}", ev.power);
///     }
/// }
/// ```
pub struct EventReader<'w, T: 'static> {
    events: &'w Events<T>,
}

impl<'w, T: 'static> EventReader<'w, T> {
    /// Iterate over events from the previous tick.
    pub fn read(&self) -> impl Iterator<Item = &T> {
        self.events.read()
    }

    /// Number of readable events.
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Returns `true` if there are no readable events.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

// SAFETY: access() reports ResRead(Events<T>). fetch() only reads the
// Events<T> resource via get_resource — no mutation occurs.
unsafe impl<T: 'static> SystemParam for EventReader<'_, T> {
    type Item<'w> = EventReader<'w, T>;

    fn access() -> Vec<Access> {
        vec![Access::ResRead(TypeId::of::<Events<T>>())]
    }

    unsafe fn fetch<'w>(world: UnsafeWorldCell) -> Self::Item<'w> {
        EventReader {
            events: unsafe { world.get_resource::<Events<T>>() },
        }
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system_param::{SystemParam, has_conflicts};
    use crate::world::World;

    #[derive(Debug, PartialEq)]
    struct DamageEvent {
        amount: u32,
    }

    #[derive(Debug, PartialEq)]
    struct SpawnEvent {
        x: f32,
    }

    // -------------------------------------------------------------------------
    // Events<T> unit tests
    // -------------------------------------------------------------------------

    #[test]
    fn events_send_and_read() {
        let mut events: Events<DamageEvent> = Events::new();

        // No events readable yet.
        assert!(events.is_empty());
        assert_eq!(events.len(), 0);

        // Send an event.
        events.send(DamageEvent { amount: 42 });

        // Not yet readable — still in current.
        assert!(events.is_empty());

        // Advance the buffer.
        events.update();

        // Now readable in previous.
        assert!(!events.is_empty());
        assert_eq!(events.len(), 1);
        let collected: Vec<_> = events.read().collect();
        assert_eq!(collected, vec![&DamageEvent { amount: 42 }]);
    }

    #[test]
    fn events_double_buffer_semantics() {
        let mut events: Events<DamageEvent> = Events::new();

        // Tick N: send event.
        events.send(DamageEvent { amount: 10 });
        events.update(); // current → previous

        // Tick N+1 start: event is in previous → readable.
        assert_eq!(events.len(), 1);

        // Tick N+1: no new events, advance again.
        events.update(); // previous is cleared, empty current stays current

        // Tick N+2 start: previous is now cleared.
        assert!(events.is_empty());
    }

    #[test]
    fn events_multiple_sends_same_tick() {
        let mut events: Events<DamageEvent> = Events::new();

        events.send(DamageEvent { amount: 1 });
        events.send(DamageEvent { amount: 2 });
        events.send(DamageEvent { amount: 3 });
        events.update();

        assert_eq!(events.len(), 3);
        let amounts: Vec<u32> = events.read().map(|e| e.amount).collect();
        assert_eq!(amounts, vec![1, 2, 3]);
    }

    // -------------------------------------------------------------------------
    // Access declaration tests
    // -------------------------------------------------------------------------

    #[test]
    fn event_writer_access_is_res_write() {
        let access = <EventWriter<'_, DamageEvent> as SystemParam>::access();
        assert_eq!(access.len(), 1);
        assert_eq!(
            access[0],
            Access::ResWrite(TypeId::of::<Events<DamageEvent>>())
        );
    }

    #[test]
    fn event_reader_access_is_res_read() {
        let access = <EventReader<'_, DamageEvent> as SystemParam>::access();
        assert_eq!(access.len(), 1);
        assert_eq!(
            access[0],
            Access::ResRead(TypeId::of::<Events<DamageEvent>>())
        );
    }

    // -------------------------------------------------------------------------
    // Conflict detection tests
    // -------------------------------------------------------------------------

    #[test]
    fn event_writer_reader_different_types_no_conflict() {
        // EventWriter<DamageEvent> and EventReader<SpawnEvent> — different
        // Events<T> TypeIds — must not conflict.
        let writer_access = <EventWriter<'_, DamageEvent> as SystemParam>::access();
        let reader_access = <EventReader<'_, SpawnEvent> as SystemParam>::access();
        assert!(!has_conflicts(&writer_access, &reader_access));
    }

    #[test]
    fn event_writer_reader_same_type_conflicts() {
        // EventWriter<DamageEvent> and EventReader<DamageEvent> both touch
        // Events<DamageEvent> — ResWrite + ResRead on the same TypeId → conflict.
        let writer_access = <EventWriter<'_, DamageEvent> as SystemParam>::access();
        let reader_access = <EventReader<'_, DamageEvent> as SystemParam>::access();
        assert!(has_conflicts(&writer_access, &reader_access));
    }

    #[test]
    fn event_reader_reader_same_type_no_conflict() {
        // Two EventReaders on the same type — ResRead + ResRead → no conflict.
        let a = <EventReader<'_, DamageEvent> as SystemParam>::access();
        let b = <EventReader<'_, DamageEvent> as SystemParam>::access();
        assert!(!has_conflicts(&a, &b));
    }

    // -------------------------------------------------------------------------
    // SystemParam fetch tests
    // -------------------------------------------------------------------------

    #[test]
    fn event_writer_fetch_and_send() {
        let mut world = World::new();
        world.add_event::<DamageEvent>();

        let cell = unsafe { UnsafeWorldCell::new(&mut world as *mut World) };
        unsafe {
            let mut writer = <EventWriter<'_, DamageEvent> as SystemParam>::fetch(cell);
            writer.send(DamageEvent { amount: 99 });
        }

        // Advance to make current → previous.
        world.update_events();

        assert_eq!(world.resource::<Events<DamageEvent>>().len(), 1);
    }

    #[test]
    fn event_reader_fetch_and_read() {
        let mut world = World::new();
        world.add_event::<DamageEvent>();

        // Send an event directly through the resource.
        world
            .resource_mut::<Events<DamageEvent>>()
            .send(DamageEvent { amount: 7 });
        world.update_events();

        let cell = unsafe { UnsafeWorldCell::new(&mut world as *mut World) };
        unsafe {
            let reader = <EventReader<'_, DamageEvent> as SystemParam>::fetch(cell);
            assert_eq!(reader.len(), 1);
            let ev = reader.read().next().unwrap();
            assert_eq!(ev.amount, 7);
        }
    }

    #[test]
    fn add_event_duplicate_no_updater_duplication() {
        let mut world = World::new();
        world.add_event::<DamageEvent>();
        world.add_event::<DamageEvent>(); // no-op

        world
            .resource_mut::<Events<DamageEvent>>()
            .send(DamageEvent { amount: 5 });
        world.update_events();

        // If the updater were duplicated, the second would clear previous.
        assert_eq!(world.resource::<Events<DamageEvent>>().len(), 1);
    }

    #[test]
    fn add_event_duplicate_does_not_drop_queued_events() {
        let mut world = World::new();
        world.add_event::<DamageEvent>();

        // Queue an event, then call add_event again — must not reset the buffer.
        world
            .resource_mut::<Events<DamageEvent>>()
            .send(DamageEvent { amount: 99 });
        world.add_event::<DamageEvent>(); // no-op

        world.update_events();
        assert_eq!(world.resource::<Events<DamageEvent>>().len(), 1);
        assert_eq!(
            world
                .resource::<Events<DamageEvent>>()
                .read()
                .next()
                .unwrap()
                .amount,
            99
        );
    }

    #[test]
    fn add_event_after_take_resource_restores_without_duplicate_updater() {
        let mut world = World::new();
        world.add_event::<DamageEvent>();

        // Remove the resource via the public API.
        let _old: Events<DamageEvent> = world.take_resource();

        // Re-register — must restore the resource and not duplicate the updater.
        world.add_event::<DamageEvent>();

        world
            .resource_mut::<Events<DamageEvent>>()
            .send(DamageEvent { amount: 77 });
        world.update_events();

        // One updater → event survives in previous. Two would clear it.
        assert_eq!(world.resource::<Events<DamageEvent>>().len(), 1);
        assert_eq!(
            world
                .resource::<Events<DamageEvent>>()
                .read()
                .next()
                .unwrap()
                .amount,
            77
        );
    }
}
