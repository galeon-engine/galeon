// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

//! UTC-based deadline scheduler for timed event firing.
//!
//! Register `(Timestamp, event)` pairs. Each tick, the scheduler fires all
//! entries where `now >= deadline`, writing them as [`Events<T>`] so game
//! systems can read fired deadlines via [`EventReader<T>`].
//!
//! # Clock
//!
//! The [`Clock`] trait provides an injectable time source. [`SystemClock`]
//! uses the real wall clock; [`TestClock`] allows deterministic tests.
//!
//! # Example
//!
//! ```rust,ignore
//! use galeon_engine::{Engine, Deadlines, Timestamp, TestClock};
//!
//! let mut engine = Engine::new();
//! let clock = TestClock::new(Timestamp::from_secs(1000));
//! engine.world_mut().insert_resource(Box::new(clock) as Box<dyn Clock>);
//! engine.world_mut().add_deadline_type::<TimedEvent>();
//!
//! let id = engine.world_mut().schedule_deadline(
//!     Timestamp::from_secs(1500),
//!     TimedEvent { entity_id: 42 },
//! );
//! ```
//!
//! [`Events<T>`]: crate::event::Events
//! [`EventReader<T>`]: crate::event::EventReader

use std::cmp::Ordering;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::time::{SystemTime, UNIX_EPOCH};

// =============================================================================
// Timestamp — microseconds since UNIX epoch
// =============================================================================

/// A point in time represented as microseconds since the UNIX epoch.
///
/// This is a lightweight, dependency-free alternative to `chrono::DateTime<Utc>`.
/// Games that use `chrono` can convert: `Timestamp::from_micros(dt.timestamp_micros())`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(i64);

impl Timestamp {
    /// Create a timestamp from microseconds since UNIX epoch.
    pub fn from_micros(us: i64) -> Self {
        Self(us)
    }

    /// Create a timestamp from seconds since UNIX epoch.
    pub fn from_secs(secs: i64) -> Self {
        Self(secs * 1_000_000)
    }

    /// Returns the value as microseconds since UNIX epoch.
    pub fn as_micros(&self) -> i64 {
        self.0
    }

    /// Returns the value as seconds since UNIX epoch (truncated).
    pub fn as_secs(&self) -> i64 {
        self.0 / 1_000_000
    }

    /// Returns the current wall-clock time.
    pub fn now() -> Self {
        let d = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before UNIX epoch");
        Self(d.as_micros() as i64)
    }
}

// =============================================================================
// Clock trait — injectable time source
// =============================================================================

/// Injectable time source for the deadline scheduler.
///
/// Implement this trait to control how the scheduler determines "now".
/// The engine provides [`SystemClock`] (real time) and [`TestClock`]
/// (manually controllable).
pub trait Clock: 'static {
    /// Returns the current time.
    fn now(&self) -> Timestamp;
}

/// Wall-clock time source using `std::time::SystemTime`.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }
}

/// Manually controllable time source for deterministic tests.
///
/// Advance time with [`set`](TestClock::set) or [`advance_secs`](TestClock::advance_secs).
pub struct TestClock {
    now: Timestamp,
}

impl TestClock {
    /// Create a test clock starting at the given time.
    pub fn new(now: Timestamp) -> Self {
        Self { now }
    }

    /// Set the current time.
    pub fn set(&mut self, now: Timestamp) {
        self.now = now;
    }

    /// Advance the clock by the given number of seconds.
    pub fn advance_secs(&mut self, secs: i64) {
        self.now = Timestamp::from_micros(self.now.as_micros() + secs * 1_000_000);
    }

    /// Advance the clock by the given number of microseconds.
    pub fn advance_micros(&mut self, us: i64) {
        self.now = Timestamp::from_micros(self.now.as_micros() + us);
    }
}

impl Clock for TestClock {
    fn now(&self) -> Timestamp {
        self.now
    }
}

// =============================================================================
// DeadlineId — unique handle for cancellation
// =============================================================================

static NEXT_DEADLINE_ID: AtomicU64 = AtomicU64::new(1);

/// Opaque handle returned by [`Deadlines::schedule`] for cancellation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DeadlineId(u64);

impl DeadlineId {
    fn next() -> Self {
        Self(NEXT_DEADLINE_ID.fetch_add(1, AtomicOrdering::Relaxed))
    }
}

// =============================================================================
// Deadlines<T> — sorted deadline storage
// =============================================================================

/// An entry in the deadline queue.
struct DeadlineEntry<T> {
    id: DeadlineId,
    deadline: Timestamp,
    event: T,
}

/// Sorted deadline storage for a single event type.
///
/// Entries are kept sorted by deadline (earliest first) for efficient
/// draining. Insert is O(log n) via binary search + insert.
///
/// Register with [`World::add_deadline_type::<T>()`] and schedule entries
/// with [`World::schedule_deadline()`].
pub struct Deadlines<T: 'static> {
    entries: Vec<DeadlineEntry<T>>,
}

impl<T: 'static> Deadlines<T> {
    /// Create an empty deadline queue.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Schedule an event to fire at the given deadline. Returns an ID for
    /// cancellation.
    pub fn schedule(&mut self, deadline: Timestamp, event: T) -> DeadlineId {
        let id = DeadlineId::next();
        let pos = self
            .entries
            .binary_search_by(|e| e.deadline.cmp(&deadline).then(Ordering::Less))
            .unwrap_or_else(|i| i);
        self.entries.insert(
            pos,
            DeadlineEntry {
                id,
                deadline,
                event,
            },
        );
        id
    }

    /// Cancel a previously scheduled deadline. Returns `true` if found.
    pub fn cancel(&mut self, id: DeadlineId) -> bool {
        if let Some(pos) = self.entries.iter().position(|e| e.id == id) {
            self.entries.remove(pos);
            true
        } else {
            false
        }
    }

    /// Drain all entries where `now >= deadline`, returning them in order.
    ///
    /// This is the batch resolution path: all overdue deadlines fire in a
    /// single tick, supporting catch-up after pause or reconnect.
    pub fn drain_overdue(&mut self, now: Timestamp) -> Vec<T> {
        // Find the partition point: entries[..split] are overdue.
        let split = self.entries.partition_point(|e| e.deadline <= now);
        if split == 0 {
            return Vec::new();
        }
        self.entries.drain(..split).map(|e| e.event).collect()
    }

    /// Returns the number of pending deadlines.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if no deadlines are pending.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns the earliest deadline, if any.
    pub fn next_deadline(&self) -> Option<Timestamp> {
        self.entries.first().map(|e| e.deadline)
    }
}

impl<T: 'static> Default for Deadlines<T> {
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

    // -- Timestamp --

    #[test]
    fn timestamp_from_secs_roundtrip() {
        let ts = Timestamp::from_secs(1000);
        assert_eq!(ts.as_secs(), 1000);
        assert_eq!(ts.as_micros(), 1_000_000_000);
    }

    #[test]
    fn timestamp_from_micros() {
        let ts = Timestamp::from_micros(123_456_789);
        assert_eq!(ts.as_micros(), 123_456_789);
        assert_eq!(ts.as_secs(), 123); // truncated
    }

    #[test]
    fn timestamp_ordering() {
        let a = Timestamp::from_secs(100);
        let b = Timestamp::from_secs(200);
        assert!(a < b);
        assert!(b > a);
        assert_eq!(a, Timestamp::from_secs(100));
    }

    #[test]
    fn timestamp_now_is_positive() {
        let ts = Timestamp::now();
        assert!(ts.as_micros() > 0);
    }

    // -- Clock impls --

    #[test]
    fn system_clock_returns_positive() {
        let clock = SystemClock;
        assert!(clock.now().as_micros() > 0);
    }

    #[test]
    fn test_clock_manual_control() {
        let mut clock = TestClock::new(Timestamp::from_secs(1000));
        assert_eq!(clock.now().as_secs(), 1000);

        clock.advance_secs(60);
        assert_eq!(clock.now().as_secs(), 1060);

        clock.set(Timestamp::from_secs(2000));
        assert_eq!(clock.now().as_secs(), 2000);
    }

    #[test]
    fn test_clock_advance_micros() {
        let mut clock = TestClock::new(Timestamp::from_micros(0));
        clock.advance_micros(500_000);
        assert_eq!(clock.now().as_micros(), 500_000);
    }

    // -- DeadlineId --

    #[test]
    fn deadline_ids_are_unique() {
        let a = DeadlineId::next();
        let b = DeadlineId::next();
        assert_ne!(a, b);
    }

    // -- Deadlines<T> --

    #[derive(Debug, PartialEq)]
    struct TestEvent(u32);

    #[test]
    fn schedule_and_drain() {
        let mut deadlines = Deadlines::new();
        deadlines.schedule(Timestamp::from_secs(100), TestEvent(1));
        deadlines.schedule(Timestamp::from_secs(200), TestEvent(2));
        deadlines.schedule(Timestamp::from_secs(300), TestEvent(3));

        assert_eq!(deadlines.len(), 3);

        // Drain at t=150: only first event fires.
        let fired = deadlines.drain_overdue(Timestamp::from_secs(150));
        assert_eq!(fired, vec![TestEvent(1)]);
        assert_eq!(deadlines.len(), 2);
    }

    #[test]
    fn drain_all_overdue_batch() {
        let mut deadlines = Deadlines::new();
        deadlines.schedule(Timestamp::from_secs(100), TestEvent(1));
        deadlines.schedule(Timestamp::from_secs(200), TestEvent(2));
        deadlines.schedule(Timestamp::from_secs(300), TestEvent(3));

        // Drain at t=300: all fire (batch reconciliation).
        let fired = deadlines.drain_overdue(Timestamp::from_secs(300));
        assert_eq!(fired, vec![TestEvent(1), TestEvent(2), TestEvent(3)]);
        assert!(deadlines.is_empty());
    }

    #[test]
    fn drain_none_overdue() {
        let mut deadlines = Deadlines::new();
        deadlines.schedule(Timestamp::from_secs(200), TestEvent(1));

        let fired = deadlines.drain_overdue(Timestamp::from_secs(100));
        assert!(fired.is_empty());
        assert_eq!(deadlines.len(), 1);
    }

    #[test]
    fn drain_empty_queue() {
        let mut deadlines: Deadlines<TestEvent> = Deadlines::new();
        let fired = deadlines.drain_overdue(Timestamp::from_secs(100));
        assert!(fired.is_empty());
    }

    #[test]
    fn cancel_removes_entry() {
        let mut deadlines = Deadlines::new();
        let id = deadlines.schedule(Timestamp::from_secs(100), TestEvent(1));
        deadlines.schedule(Timestamp::from_secs(200), TestEvent(2));

        assert!(deadlines.cancel(id));
        assert_eq!(deadlines.len(), 1);

        let fired = deadlines.drain_overdue(Timestamp::from_secs(300));
        assert_eq!(fired, vec![TestEvent(2)]);
    }

    #[test]
    fn cancel_nonexistent_returns_false() {
        let mut deadlines: Deadlines<TestEvent> = Deadlines::new();
        let id = DeadlineId::next();
        assert!(!deadlines.cancel(id));
    }

    #[test]
    fn cancel_already_cancelled_returns_false() {
        let mut deadlines = Deadlines::new();
        let id = deadlines.schedule(Timestamp::from_secs(100), TestEvent(1));
        assert!(deadlines.cancel(id));
        assert!(!deadlines.cancel(id));
    }

    #[test]
    fn sorted_insertion_order() {
        let mut deadlines = Deadlines::new();
        // Insert out of order.
        deadlines.schedule(Timestamp::from_secs(300), TestEvent(3));
        deadlines.schedule(Timestamp::from_secs(100), TestEvent(1));
        deadlines.schedule(Timestamp::from_secs(200), TestEvent(2));

        // Drain all — should come out in deadline order.
        let fired = deadlines.drain_overdue(Timestamp::from_secs(400));
        assert_eq!(fired, vec![TestEvent(1), TestEvent(2), TestEvent(3)]);
    }

    #[test]
    fn same_deadline_fires_all() {
        let mut deadlines = Deadlines::new();
        deadlines.schedule(Timestamp::from_secs(100), TestEvent(1));
        deadlines.schedule(Timestamp::from_secs(100), TestEvent(2));

        let fired = deadlines.drain_overdue(Timestamp::from_secs(100));
        assert_eq!(fired.len(), 2);
    }

    #[test]
    fn next_deadline_returns_earliest() {
        let mut deadlines = Deadlines::new();
        assert!(deadlines.next_deadline().is_none());

        deadlines.schedule(Timestamp::from_secs(300), TestEvent(3));
        deadlines.schedule(Timestamp::from_secs(100), TestEvent(1));

        assert_eq!(deadlines.next_deadline(), Some(Timestamp::from_secs(100)));
    }

    // -- World integration tests --

    #[test]
    fn world_add_deadline_type_is_idempotent() {
        let mut world = crate::world::World::new();
        world.add_deadline_type::<TestEvent>();
        world.add_deadline_type::<TestEvent>(); // no-op
        assert!(world.try_resource::<Deadlines<TestEvent>>().is_some());
    }

    #[test]
    fn world_schedule_and_drain() {
        let mut world = crate::world::World::new();
        world.add_deadline_type::<TestEvent>();

        world.schedule_deadline(Timestamp::from_secs(100), TestEvent(1));
        world.schedule_deadline(Timestamp::from_secs(200), TestEvent(2));

        // Drain at t=150: fires first, writes to Events<TestEvent>.
        world.drain_deadlines::<TestEvent>(Timestamp::from_secs(150));

        // Events are in current buffer. Advance to make them readable.
        world.update_events();

        let events = world.resource::<crate::event::Events<TestEvent>>();
        assert_eq!(events.len(), 1);
        let fired: Vec<_> = events.read().collect();
        assert_eq!(fired[0].0, 1);

        // Second deadline still pending.
        assert_eq!(world.resource::<Deadlines<TestEvent>>().len(), 1);
    }

    #[test]
    fn world_cancel_deadline() {
        let mut world = crate::world::World::new();
        world.add_deadline_type::<TestEvent>();

        let id = world.schedule_deadline(Timestamp::from_secs(100), TestEvent(1));
        assert!(world.cancel_deadline::<TestEvent>(id));
        assert!(world.resource::<Deadlines<TestEvent>>().is_empty());
    }

    #[test]
    fn world_batch_reconciliation() {
        let mut world = crate::world::World::new();
        world.add_deadline_type::<TestEvent>();

        // Schedule 3 deadlines in the past.
        world.schedule_deadline(Timestamp::from_secs(100), TestEvent(1));
        world.schedule_deadline(Timestamp::from_secs(200), TestEvent(2));
        world.schedule_deadline(Timestamp::from_secs(300), TestEvent(3));

        // Drain at t=1000: all fire at once (catch-up after pause/reconnect).
        world.drain_deadlines::<TestEvent>(Timestamp::from_secs(1000));
        world.update_events();

        let events = world.resource::<crate::event::Events<TestEvent>>();
        assert_eq!(events.len(), 3);
        let values: Vec<u32> = events.read().map(|e| e.0).collect();
        assert_eq!(values, vec![1, 2, 3]);
    }

    #[test]
    fn world_drain_with_test_clock() {
        let mut world = crate::world::World::new();
        world.add_deadline_type::<TestEvent>();

        let mut clock = TestClock::new(Timestamp::from_secs(0));

        world.schedule_deadline(Timestamp::from_secs(60), TestEvent(1));
        world.schedule_deadline(Timestamp::from_secs(120), TestEvent(2));

        // t=0: nothing fires.
        world.drain_deadlines::<TestEvent>(clock.now());
        world.update_events();
        assert!(
            world
                .resource::<crate::event::Events<TestEvent>>()
                .is_empty()
        );

        // Advance to t=60: first fires.
        clock.advance_secs(60);
        world.drain_deadlines::<TestEvent>(clock.now());
        world.update_events();
        assert_eq!(world.resource::<crate::event::Events<TestEvent>>().len(), 1);

        // Advance to t=120: second fires.
        clock.advance_secs(60);
        world.drain_deadlines::<TestEvent>(clock.now());
        world.update_events();
        assert_eq!(world.resource::<crate::event::Events<TestEvent>>().len(), 1);

        // All drained.
        assert!(world.resource::<Deadlines<TestEvent>>().is_empty());
    }

    // -- drain_all_deadlines integration --

    #[test]
    fn drain_all_deadlines_with_clock_resource() {
        let mut world = crate::world::World::new();
        world.add_deadline_type::<TestEvent>();
        world
            .insert_resource(Box::new(TestClock::new(Timestamp::from_secs(150))) as Box<dyn Clock>);

        world.schedule_deadline(Timestamp::from_secs(100), TestEvent(1));
        world.schedule_deadline(Timestamp::from_secs(200), TestEvent(2));

        // drain_all_deadlines reads the Clock, fires overdue.
        world.drain_all_deadlines();

        // Events in current. Advance to make readable.
        world.update_events();

        let events = world.resource::<crate::event::Events<TestEvent>>();
        assert_eq!(events.len(), 1);
        assert_eq!(events.read().next().unwrap().0, 1);
    }

    #[test]
    fn drain_all_deadlines_no_clock_is_noop() {
        let mut world = crate::world::World::new();
        world.add_deadline_type::<TestEvent>();
        world.schedule_deadline(Timestamp::from_secs(100), TestEvent(1));

        // No Clock resource — should not panic, should not drain.
        world.drain_all_deadlines();
        assert_eq!(world.resource::<Deadlines<TestEvent>>().len(), 1);
    }

    // -- Schedule integration: same-tick delivery --

    #[test]
    fn schedule_run_fires_deadlines_readable_same_tick() {
        use crate::schedule::Schedule;

        let mut world = crate::world::World::new();
        world.add_deadline_type::<TestEvent>();
        world
            .insert_resource(Box::new(TestClock::new(Timestamp::from_secs(200))) as Box<dyn Clock>);

        // Schedule a deadline in the past (should fire on first run).
        world.schedule_deadline(Timestamp::from_secs(100), TestEvent(42));

        // A system that reads fired deadline events.
        world.insert_resource(0_u32); // counter
        fn count_fired(
            reader: crate::event::EventReader<'_, TestEvent>,
            mut counter: crate::system_param::ResMut<'_, u32>,
        ) {
            for _ in reader.read() {
                *counter += 1;
            }
        }

        let mut schedule = Schedule::new();
        schedule.add_system::<(
            crate::event::EventReader<'_, TestEvent>,
            crate::system_param::ResMut<'_, u32>,
        )>("update", "count_fired", count_fired);

        // Run the schedule once.
        schedule.run(&mut world);

        // The system should have seen the fired deadline event THIS tick.
        assert_eq!(*world.resource::<u32>(), 1);
        // Deadline queue should be empty.
        assert!(world.resource::<Deadlines<TestEvent>>().is_empty());
    }

    #[test]
    fn schedule_run_multiple_deadline_types() {
        use crate::schedule::Schedule;

        #[derive(Debug, PartialEq)]
        struct OtherEvent(u32);

        let mut world = crate::world::World::new();
        world.add_deadline_type::<TestEvent>();
        world.add_deadline_type::<OtherEvent>();
        world
            .insert_resource(Box::new(TestClock::new(Timestamp::from_secs(500))) as Box<dyn Clock>);

        world.schedule_deadline(Timestamp::from_secs(100), TestEvent(1));
        world.schedule_deadline(Timestamp::from_secs(200), OtherEvent(2));

        // Both should drain automatically.
        let mut schedule = Schedule::new();
        schedule.run(&mut world);

        // Both deadline queues empty.
        assert!(world.resource::<Deadlines<TestEvent>>().is_empty());
        assert!(world.resource::<Deadlines<OtherEvent>>().is_empty());

        // Both event types were fired (in current, now moved to previous).
        assert_eq!(world.resource::<crate::event::Events<TestEvent>>().len(), 1);
        assert_eq!(
            world.resource::<crate::event::Events<OtherEvent>>().len(),
            1
        );
    }
}
