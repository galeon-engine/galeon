// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

use std::any::TypeId;
use std::collections::HashSet;

use crate::archetype::{ArchetypeLayout, ArchetypeStore, EntityLocation};
use crate::commands::CommandBuffer;
use crate::component::Component;
use crate::deadline::{Clock, DeadlineId, Deadlines, Timestamp};
use crate::entity::{Entity, EntityMetaStore};
use crate::event::Events;
use crate::query::{
    AddedIter, ChangedIter, Query2Iter, Query2MutIter, Query3Iter, Query3MutIter, QueryFilter,
    QueryIter, QueryIterMut, QuerySpec, QuerySpecMut,
};
use crate::resource::Resources;

// =============================================================================
// Bundle trait
// =============================================================================

/// A bundle of components that can be spawned together.
///
/// Implemented for tuples of components up to 8 elements.
/// Provides type IDs for archetype layout computation and column registration.
pub trait Bundle: 'static {
    /// Sorted type IDs for all component types in this bundle.
    ///
    /// Duplicate component types are rejected to preserve the invariant that
    /// each archetype column has exactly one value per entity row.
    fn type_ids() -> Vec<TypeId>;

    /// Register column factories for all component types in this bundle.
    fn register_columns(store: &mut ArchetypeStore);

    /// Push all component values into the archetype's columns.
    ///
    /// The archetype must contain columns for all types in this bundle.
    fn push_into_columns(self, archetype: &mut crate::archetype::Archetype);
}

// Implement Bundle for single component.
impl<A: Component> Bundle for (A,) {
    fn type_ids() -> Vec<TypeId> {
        vec![TypeId::of::<A>()]
    }

    fn register_columns(store: &mut ArchetypeStore) {
        store.register_column::<A>();
    }

    fn push_into_columns(self, archetype: &mut crate::archetype::Archetype) {
        archetype.column_mut::<A>().unwrap().push(self.0);
    }
}

// Implement Bundle for tuples of 2-8 components via macro.
macro_rules! impl_bundle {
    ($($t:ident),+) => {
        #[allow(non_snake_case)]
        impl<$($t: Component),+> Bundle for ($($t,)+) {
            fn type_ids() -> Vec<TypeId> {
                let mut ids = vec![$(TypeId::of::<$t>()),+];
                let original_len = ids.len();
                ids.sort();
                ids.dedup();
                assert_eq!(
                    ids.len(),
                    original_len,
                    "duplicate component types are not allowed in a Bundle"
                );
                ids
            }

            fn register_columns(store: &mut ArchetypeStore) {
                $(store.register_column::<$t>();)+
            }

            fn push_into_columns(self, archetype: &mut crate::archetype::Archetype) {
                let ($($t,)+) = self;
                $(archetype.column_mut::<$t>().unwrap().push($t);)+
            }
        }
    };
}

impl_bundle!(A, B);
impl_bundle!(A, B, C);
impl_bundle!(A, B, C, D);
impl_bundle!(A, B, C, D, E);
impl_bundle!(A, B, C, D, E, F);
impl_bundle!(A, B, C, D, E, F, G);
impl_bundle!(A, B, C, D, E, F, G, H);

// =============================================================================
// UnsafeWorldCell
// =============================================================================

/// A raw pointer wrapper around `World` that provides field-level access
/// without creating intermediate `&World` or `&mut World` references.
///
/// This eliminates the Stacked Borrows aliasing UB that occurs when
/// multiple `SystemParam::fetch()` calls create overlapping shared/exclusive
/// world references. Instead of `(*world).resource::<T>()` (which creates
/// `&World`), the cell uses `addr_of!` to reach individual fields directly.
///
/// # Safety Contract
///
/// The caller must ensure:
/// - The pointed-to `World` is valid for the `'w` lifetime of any returned
///   reference.
/// - No two accessors create aliasing mutable references to the same
///   underlying data. This is guaranteed by the conflict detection system
///   at system registration time.
#[derive(Copy, Clone)]
pub struct UnsafeWorldCell(*mut World);

impl UnsafeWorldCell {
    /// Create a new cell from a raw world pointer.
    ///
    /// # Safety
    ///
    /// The pointer must be non-null, well-aligned, and the `World` must
    /// live for the duration of all accesses through this cell.
    #[inline]
    pub unsafe fn new(world: *mut World) -> Self {
        debug_assert!(!world.is_null());
        Self(world)
    }

    /// Get a shared reference to a resource without creating `&World`.
    ///
    /// Both `get_resource` and `get_resource_mut` create only `&Resources`
    /// (shared) via `addr_of!`, using `UnsafeCell`-based interior mutability
    /// for the write path. This eliminates `&Resources` / `&mut Resources`
    /// overlap when `Res<A>` and `ResMut<B>` are fetched concurrently.
    ///
    /// # Safety
    ///
    /// - The resource of type `T` must exist in the world.
    /// - No mutable reference to the same resource may exist concurrently.
    #[inline]
    pub unsafe fn get_resource<'w, T: 'static>(self) -> &'w T {
        // SAFETY: addr_of! avoids creating &World. The resulting &Resources
        // is shared — combined with get_unchecked (which also uses &self),
        // no &mut Resources is ever created on this path.
        unsafe {
            let resources_ptr: *const Resources = std::ptr::addr_of!((*self.0).resources);
            (*resources_ptr).get_unchecked::<T>()
        }
    }

    /// Get a mutable reference to a resource without creating `&mut World`
    /// or `&mut Resources`.
    ///
    /// Uses `addr_of!` (not `addr_of_mut!`) to create `&Resources` (shared),
    /// then reaches into the `UnsafeCell`-wrapped value for interior
    /// mutability. This ensures `Res<A>` + `ResMut<B>` never produce
    /// overlapping `&Resources` / `&mut Resources`.
    ///
    /// # Safety
    ///
    /// - The resource of type `T` must exist in the world.
    /// - No other reference (shared or mutable) to the same resource may
    ///   exist concurrently.
    #[inline]
    pub unsafe fn get_resource_mut<'w, T: 'static>(self) -> &'w mut T {
        // SAFETY: addr_of! avoids creating &World or &mut World. We create
        // only &Resources (shared). get_mut_unchecked uses UnsafeCell for
        // interior mutability — caller guarantees exclusive resource access.
        unsafe {
            let resources_ptr: *const Resources = std::ptr::addr_of!((*self.0).resources);
            (*resources_ptr).get_mut_unchecked::<T>()
        }
    }

    /// Get shared access to the archetype store without creating `&World`.
    ///
    /// # Safety
    ///
    /// No mutable reference to the archetype store may exist concurrently
    /// (i.e., no `QueryMut` for any component type may be live).
    #[inline]
    pub unsafe fn archetypes<'w>(self) -> &'w ArchetypeStore {
        // SAFETY: Caller guarantees no mutable archetype access exists.
        unsafe {
            let ptr: *const ArchetypeStore = std::ptr::addr_of!((*self.0).archetypes);
            &*ptr
        }
    }

    /// Get a raw mutable pointer to the archetype store without creating
    /// `&mut World` or `&mut ArchetypeStore`.
    ///
    /// Returns `*mut ArchetypeStore` (not a reference) so that
    /// `QueryIterMut` can be constructed without creating `&mut ArchetypeStore`,
    /// eliminating the `&ArchetypeStore` / `&mut ArchetypeStore` overlap
    /// when `Query<A>` and `QueryMut<B>` are fetched concurrently.
    ///
    /// # Safety
    ///
    /// - The caller must ensure no other reference to the archetype store
    ///   exists when writing through this pointer.
    /// - The conflict detection system guarantees disjoint component access.
    #[inline]
    pub unsafe fn archetypes_mut_ptr(self) -> *mut ArchetypeStore {
        // SAFETY: addr_of_mut! computes the field offset without creating
        // any intermediate reference.
        unsafe { std::ptr::addr_of_mut!((*self.0).archetypes) }
    }

    /// Get a mutable reference to the command buffer without creating
    /// `&mut World`.
    ///
    /// # Safety
    ///
    /// - No other reference to the command buffer may exist concurrently.
    /// - The command buffer is a separate field from resources and archetypes,
    ///   so this does not alias other `UnsafeWorldCell` accessors.
    #[inline]
    pub unsafe fn commands_mut<'w>(self) -> &'w mut CommandBuffer {
        // SAFETY: addr_of_mut! avoids creating &mut World. Caller guarantees
        // exclusive access to the command buffer field.
        unsafe {
            let ptr: *mut CommandBuffer = std::ptr::addr_of_mut!((*self.0).commands);
            &mut *ptr
        }
    }

    /// Read the current change-detection tick without creating `&World`.
    ///
    /// # Safety
    ///
    /// The `World` must be valid for the duration of this call.
    #[inline]
    pub unsafe fn change_tick(self) -> u64 {
        unsafe {
            let ptr: *const u64 = std::ptr::addr_of!((*self.0).change_tick);
            *ptr
        }
    }
}

// =============================================================================
// World
// =============================================================================

/// Type-erased closure that advances a single `Events<T>` double buffer.
type EventUpdater = Box<dyn Fn(&mut World)>;

/// Type-erased closure that drains overdue deadlines for a single `Deadlines<T>`.
type DeadlineDrainer = Box<dyn Fn(&mut World, Timestamp)>;

/// The ECS world: owns entities, archetype storage, resources, and commands.
pub struct World {
    meta: EntityMetaStore,
    archetypes: ArchetypeStore,
    resources: Resources,
    commands: CommandBuffer,
    /// Monotonically increasing change-detection tick. Starts at 1; tick 0 is
    /// the sentinel meaning "never observed". Incremented by `advance_tick()`.
    change_tick: u64,
    /// Type-erased closures that advance each registered `Events<T>` buffer.
    ///
    /// Populated by [`World::add_event::<T>()`]. Each closure calls
    /// `Events::<T>::update()` on the corresponding resource. Called by
    /// [`World::update_events()`] at the start of every `Schedule::run()`.
    event_updaters: Vec<EventUpdater>,
    /// TypeIds of event types that have been registered via `add_event`.
    /// Guards against duplicate updater registration even if the `Events<T>`
    /// resource is removed and re-added.
    registered_events: HashSet<TypeId>,
    /// Type-erased closures that drain overdue deadlines for each registered
    /// `Deadlines<T>`. Called by [`World::drain_all_deadlines()`] at the
    /// start of every `Schedule::run()`, *before* `update_events()`.
    deadline_drainers: Vec<DeadlineDrainer>,
    /// TypeIds of deadline types registered via `add_deadline_type`.
    registered_deadlines: HashSet<TypeId>,
}

impl World {
    /// Create an empty world with a single empty archetype.
    pub fn new() -> Self {
        let mut archetypes = ArchetypeStore::new();
        // Create the "void" archetype for entities with no components.
        archetypes.get_or_create(ArchetypeLayout::empty());
        Self {
            meta: EntityMetaStore::new(),
            archetypes,
            resources: Resources::new(),
            commands: CommandBuffer::new(),
            change_tick: 1,
            event_updaters: Vec::new(),
            registered_events: HashSet::new(),
            deadline_drainers: Vec::new(),
            registered_deadlines: HashSet::new(),
        }
    }

    // -------------------------------------------------------------------------
    // Change-detection tick
    // -------------------------------------------------------------------------

    /// The current change-detection tick. Starts at 1; 0 is the sentinel.
    pub fn change_tick(&self) -> u64 {
        self.change_tick
    }

    /// Advance the change-detection tick by one. Returns the new tick value.
    pub fn advance_tick(&mut self) -> u64 {
        self.change_tick += 1;
        self.change_tick
    }

    // -------------------------------------------------------------------------
    // Entity lifecycle
    // -------------------------------------------------------------------------

    /// Spawn an entity with the given component bundle.
    pub fn spawn<B: Bundle>(&mut self, bundle: B) -> Entity {
        let type_ids = B::type_ids();
        let entity = self.meta.alloc();
        B::register_columns(&mut self.archetypes);
        let layout = ArchetypeLayout::from_type_ids(&type_ids);
        let arch_id = self.archetypes.get_or_create(layout);
        let tick = self.change_tick;
        let arch = self.archetypes.get_mut(arch_id);
        bundle.push_into_columns(arch);
        let row = arch.push_entity(entity);
        // Stamp all columns: newly spawned entity is both added and changed.
        arch.stamp_all_ticks(row, tick, tick);
        self.meta.set_location(
            entity,
            EntityLocation {
                archetype_id: arch_id,
                row,
            },
        );
        entity
    }

    /// Despawn an entity, removing all its components.
    pub fn despawn(&mut self, entity: Entity) -> bool {
        let Some(loc) = self.meta.get_location(entity) else {
            return false;
        };

        let arch = self.archetypes.get_mut(loc.archetype_id);
        let (_removed, moved) = arch.swap_remove_entity(loc.row as usize);

        // If another entity was swapped into the removed row, update its location.
        if let Some(moved_entity) = moved {
            self.meta.set_location(
                moved_entity,
                EntityLocation {
                    archetype_id: loc.archetype_id,
                    row: loc.row,
                },
            );
        }

        self.meta.dealloc(entity);
        true
    }

    /// Check whether an entity is alive.
    pub fn is_alive(&self, entity: Entity) -> bool {
        self.meta.is_alive(entity)
    }

    // -------------------------------------------------------------------------
    // Resources
    // -------------------------------------------------------------------------

    /// Insert a resource (world-global singleton).
    pub fn insert_resource<T: 'static>(&mut self, value: T) {
        self.resources.insert(value);
    }

    /// Get a reference to a resource. Panics if not present.
    pub fn resource<T: 'static>(&self) -> &T {
        self.resources.get::<T>()
    }

    /// Get a mutable reference to a resource. Panics if not present.
    pub fn resource_mut<T: 'static>(&mut self) -> &mut T {
        self.resources.get_mut::<T>()
    }

    /// Try to get a reference to a resource. Returns `None` if not present.
    pub fn try_resource<T: 'static>(&self) -> Option<&T> {
        self.resources.try_get::<T>()
    }

    /// Remove and return a resource. Panics if not present.
    pub fn take_resource<T: 'static>(&mut self) -> T {
        self.resources.take::<T>()
    }

    /// Try to remove and return a resource. Returns `None` if not present.
    pub fn try_take_resource<T: 'static>(&mut self) -> Option<T> {
        self.resources.try_take::<T>()
    }

    // -------------------------------------------------------------------------
    // Commands
    // -------------------------------------------------------------------------

    /// Drain and apply all queued commands.
    ///
    /// Called automatically between schedule stages. Can also be called
    /// manually for setup code that uses deferred mutations.
    pub fn apply_commands(&mut self) {
        // Take the queue out to avoid borrowing self.commands while
        // executing commands that need &mut World.
        let commands = self.commands.take();
        for cmd in commands {
            cmd(self);
        }
    }

    /// Get a mutable reference to the command buffer.
    ///
    /// This is the low-level escape hatch for tests and setup code. Systems
    /// should use the [`Commands`](crate::commands::Commands) system parameter.
    pub fn command_buffer_mut(&mut self) -> &mut CommandBuffer {
        &mut self.commands
    }

    // -------------------------------------------------------------------------
    // Events
    // -------------------------------------------------------------------------

    /// Register an event type and insert its `Events<T>` resource.
    ///
    /// Must be called before any system uses `EventWriter<T>` or
    /// `EventReader<T>`. Idempotent:
    ///
    /// - **First call**: inserts `Events<T>` resource and registers the
    ///   updater closure.
    /// - **Duplicate call (resource still exists)**: no-op — does not reset
    ///   queued events or duplicate the updater.
    /// - **Re-call after resource removal**: restores a fresh `Events<T>`
    ///   resource without duplicating the updater.
    pub fn add_event<T: 'static>(&mut self) {
        if self.registered_events.insert(TypeId::of::<Events<T>>()) {
            // First registration: insert resource + updater.
            self.resources.insert(Events::<T>::new());
            self.event_updaters.push(Box::new(|world: &mut World| {
                world.resources.get_mut::<Events<T>>().update();
            }));
        } else if !self.resources.contains::<Events<T>>() {
            // Resource was removed externally — restore it. Updater already
            // exists so we only need the resource back.
            self.resources.insert(Events::<T>::new());
        }
    }

    /// Advance all registered event buffers.
    ///
    /// Swaps each `Events<T>`'s `current` buffer into `previous` and clears
    /// `current`. Called automatically at the start of every
    /// [`Schedule::run()`](crate::schedule::Schedule::run) so that events
    /// written in tick N are readable in tick N+1.
    pub fn update_events(&mut self) {
        // SAFETY: We must call the updaters without holding a borrow on
        // `event_updaters` while also needing `&mut self` inside each closure.
        // We swap the vec out, call each closure, then swap it back.
        //
        // INVARIANT: EventUpdater closures MUST only access `self.resources`.
        // They must never touch `self.event_updaters` (which is empty during
        // this loop due to mem::take), `self.archetypes`, `self.commands`, or
        // any other World field. This invariant is maintained by construction —
        // all closures registered by `add_event` only call
        // `world.resources.get_mut::<Events<T>>().update()`.
        let mut updaters = std::mem::take(&mut self.event_updaters);
        for updater in &updaters {
            updater(self);
        }
        // Restore the updaters vec (reuse the allocation).
        std::mem::swap(&mut self.event_updaters, &mut updaters);
    }

    // -------------------------------------------------------------------------
    // Deadlines
    // -------------------------------------------------------------------------

    /// Register a deadline event type and insert its `Deadlines<T>` and
    /// `Events<T>` resources.
    ///
    /// Must be called before scheduling deadlines of type `T`. Idempotent.
    /// Registers a drainer closure so that [`drain_all_deadlines()`](World::drain_all_deadlines)
    /// automatically fires overdue deadlines of this type.
    pub fn add_deadline_type<T: 'static>(&mut self) {
        if self.try_resource::<Deadlines<T>>().is_none() {
            self.insert_resource(Deadlines::<T>::new());
        }
        // Ensure the Events<T> resource exists for fired deadline delivery.
        self.add_event::<T>();
        // Register the drainer (once per type).
        if self
            .registered_deadlines
            .insert(TypeId::of::<Deadlines<T>>())
        {
            self.deadline_drainers
                .push(Box::new(|world: &mut World, now: Timestamp| {
                    world.drain_deadlines::<T>(now);
                }));
        }
    }

    /// Schedule an event to fire when `now >= deadline`.
    ///
    /// Returns a [`DeadlineId`] for cancellation. The event type must have
    /// been registered with [`add_deadline_type::<T>()`](World::add_deadline_type).
    ///
    /// # Panics
    ///
    /// Panics if `Deadlines<T>` has not been registered.
    pub fn schedule_deadline<T: 'static>(&mut self, deadline: Timestamp, event: T) -> DeadlineId {
        self.resource_mut::<Deadlines<T>>()
            .schedule(deadline, event)
    }

    /// Cancel a previously scheduled deadline.
    ///
    /// Returns `true` if the deadline was found and removed.
    ///
    /// # Panics
    ///
    /// Panics if `Deadlines<T>` has not been registered.
    pub fn cancel_deadline<T: 'static>(&mut self, id: DeadlineId) -> bool {
        self.resource_mut::<Deadlines<T>>().cancel(id)
    }

    /// Drain all overdue deadlines of type `T` and write them as events.
    ///
    /// Fires all entries where `now >= deadline`, supporting batch
    /// reconciliation. Can be called manually for a single type, but
    /// prefer [`drain_all_deadlines()`](World::drain_all_deadlines) which
    /// drains all registered types automatically.
    ///
    /// # Panics
    ///
    /// Panics if `Deadlines<T>` or `Events<T>` has not been registered.
    pub fn drain_deadlines<T: 'static>(&mut self, now: Timestamp) {
        let fired = self.resource_mut::<Deadlines<T>>().drain_overdue(now);
        let events = self.resource_mut::<Events<T>>();
        for event in fired {
            events.send(event);
        }
    }

    /// Drain all overdue deadlines for every registered deadline type.
    ///
    /// Reads the [`Clock`] resource to determine "now", then calls
    /// [`drain_deadlines::<T>(now)`](World::drain_deadlines) for each type
    /// registered via [`add_deadline_type::<T>()`](World::add_deadline_type).
    ///
    /// Called automatically by [`Schedule::run()`](crate::schedule::Schedule::run)
    /// *before* [`update_events()`](World::update_events), so that fired
    /// deadline events are readable by `EventReader<T>` in the same tick.
    ///
    /// If no `Clock` resource is present, this is a no-op (deadlines are
    /// only active when a clock is installed).
    pub fn drain_all_deadlines(&mut self) {
        // Read the clock. If no clock resource, skip silently.
        let now = match self.resources.try_get::<Box<dyn Clock>>() {
            Some(clock) => clock.now(),
            None => return,
        };

        // Same swap-out pattern as update_events: take the drainers vec,
        // call each one, then restore it.
        let mut drainers = std::mem::take(&mut self.deadline_drainers);
        for drainer in &drainers {
            drainer(self, now);
        }
        std::mem::swap(&mut self.deadline_drainers, &mut drainers);
    }

    // -------------------------------------------------------------------------
    // Component access
    // -------------------------------------------------------------------------

    /// Get a component for an entity.
    pub fn get<T: Component>(&self, entity: Entity) -> Option<&T> {
        self.one::<&T>(entity)
    }

    /// Get a mutable component for an entity.
    pub fn get_mut<T: Component>(&mut self, entity: Entity) -> Option<&mut T> {
        self.one_mut::<&mut T>(entity)
    }

    /// Fetch a typed query item for a single entity.
    pub fn one<Q: QuerySpec>(&self, entity: Entity) -> Option<Q::Item<'_>> {
        let loc = self.meta.get_location(entity)?;
        let arch = self.archetypes.get(loc.archetype_id);
        if !Q::matches(arch.layout()) {
            return None;
        }
        let state = Q::init_state(arch)?;
        Some(Q::fetch(&state, loc.row as usize))
    }

    /// Fetch a typed mutable query item for a single entity.
    pub fn one_mut<Q: QuerySpecMut>(&mut self, entity: Entity) -> Option<Q::Item<'_>> {
        let loc = self.meta.get_location(entity)?;
        let tick = self.change_tick;
        let arch = self.archetypes.get_mut(loc.archetype_id);
        if !Q::matches(arch.layout()) {
            return None;
        }
        let mut state = Q::init_state(arch, tick)?;
        let row = loc.row as usize;
        // SAFETY: `loc.row` identifies one concrete row in the matched
        // archetype, so returning the fetched mutable references cannot alias
        // any other result from this method call.
        let item = unsafe { Q::fetch(&mut state, row) };
        // SAFETY: changed_ticks pointers in state are valid for this row.
        unsafe { Q::stamp_changed(&mut state, row) };
        Some(item)
    }

    // -------------------------------------------------------------------------
    // Component mutations (archetype migration)
    // -------------------------------------------------------------------------

    /// Insert a component into an entity. If the entity already has this
    /// component type, the value is overwritten in place. Otherwise, the
    /// entity migrates to an archetype that includes the new component.
    ///
    /// # Change detection
    ///
    /// When overwriting an existing component, only `changed_tick` is stamped
    /// at the current tick. `added_tick` is preserved from the original
    /// insertion (typically `spawn`). Use `query_added` to detect newly added
    /// components; it will not fire for overwrites.
    pub fn insert<C: Component>(&mut self, entity: Entity, value: C) {
        let Some(loc) = self.meta.get_location(entity) else {
            return; // dead entity
        };

        let src_arch_id = loc.archetype_id;
        let row = loc.row as usize;

        // If archetype already has C, overwrite in place.
        if self
            .archetypes
            .get(src_arch_id)
            .layout()
            .contains(TypeId::of::<C>())
        {
            let tick = self.change_tick;
            let arch = self.archetypes.get_mut(src_arch_id);
            if let Some(col) = arch.column_mut::<C>() {
                if let Some(slot) = col.get_mut(row) {
                    *slot = value;
                }
                col.set_changed_tick(row, tick);
            }
            return;
        }

        // Register column factory for C.
        self.archetypes.register_column::<C>();

        // Compute target archetype.
        let new_layout = self
            .archetypes
            .get(src_arch_id)
            .layout()
            .with_added(TypeId::of::<C>());
        let dst_arch_id = self.archetypes.get_or_create(new_layout);

        // Collect source layout type IDs before mutable borrow.
        let src_type_ids: Vec<TypeId> = self.archetypes.get(src_arch_id).layout().iter().collect();

        let tick = self.change_tick;

        // Move all existing columns from source to destination.
        // Ticks are preserved by move_to.
        let (src_arch, dst_arch) = self.archetypes.get_two_mut(src_arch_id, dst_arch_id);
        for &tid in &src_type_ids {
            let src_col = src_arch.column_raw_mut(tid).unwrap();
            let dst_col = dst_arch.column_raw_mut(tid).unwrap();
            src_col.move_to(row, dst_col);
        }

        // Push the new component into the destination.
        dst_arch.column_mut::<C>().unwrap().push(value);

        // Swap-remove entity entry from source (column data already moved).
        let (_removed, moved) = src_arch.swap_remove_entity_entry(row);

        // Push entity into destination.
        let new_row = dst_arch.push_entity(entity);

        // Stamp ticks on the newly added component C (added + changed).
        dst_arch.stamp_column_ticks(TypeId::of::<C>(), new_row, tick, tick);

        // Update locations.
        self.meta.set_location(
            entity,
            EntityLocation {
                archetype_id: dst_arch_id,
                row: new_row,
            },
        );
        if let Some(moved_entity) = moved {
            self.meta.set_location(
                moved_entity,
                EntityLocation {
                    archetype_id: src_arch_id,
                    row: loc.row,
                },
            );
        }
    }

    /// Remove a component from an entity. If the entity doesn't have this
    /// component, this is a no-op. Otherwise, the entity migrates to an
    /// archetype without the component.
    pub fn remove<C: Component>(&mut self, entity: Entity) {
        let Some(loc) = self.meta.get_location(entity) else {
            return; // dead entity
        };

        let src_arch_id = loc.archetype_id;
        let row = loc.row as usize;

        // If archetype doesn't have C, no-op.
        if !self
            .archetypes
            .get(src_arch_id)
            .layout()
            .contains(TypeId::of::<C>())
        {
            return;
        }

        // Compute target archetype (without C).
        let new_layout = self
            .archetypes
            .get(src_arch_id)
            .layout()
            .with_removed(TypeId::of::<C>());
        let dst_arch_id = self.archetypes.get_or_create(new_layout);

        // Collect source layout type IDs.
        let src_type_ids: Vec<TypeId> = self.archetypes.get(src_arch_id).layout().iter().collect();

        let c_type_id = TypeId::of::<C>();

        // Move/drop columns.
        let (src_arch, dst_arch) = self.archetypes.get_two_mut(src_arch_id, dst_arch_id);
        for &tid in &src_type_ids {
            let src_col = src_arch.column_raw_mut(tid).unwrap();
            if tid == c_type_id {
                // Drop the removed component's data.
                src_col.swap_remove_and_drop(row);
            } else {
                // Move to destination.
                let dst_col = dst_arch.column_raw_mut(tid).unwrap();
                src_col.move_to(row, dst_col);
            }
        }

        // Swap-remove entity entry from source.
        let (_removed, moved) = src_arch.swap_remove_entity_entry(row);

        // Push entity into destination.
        let new_row = dst_arch.push_entity(entity);

        // Update locations.
        self.meta.set_location(
            entity,
            EntityLocation {
                archetype_id: dst_arch_id,
                row: new_row,
            },
        );
        if let Some(moved_entity) = moved {
            self.meta.set_location(
                moved_entity,
                EntityLocation {
                    archetype_id: src_arch_id,
                    row: loc.row,
                },
            );
        }
    }

    // -------------------------------------------------------------------------
    // Queries
    // -------------------------------------------------------------------------

    /// Query all entities matching `Q`.
    pub fn query<Q: QuerySpec>(&self) -> QueryIter<'_, Q> {
        QueryIter::new(&self.archetypes)
    }

    /// Query all entities matching `Q` and `F`.
    pub fn query_filtered<Q: QuerySpec, F: QueryFilter>(&self) -> QueryIter<'_, Q, F> {
        QueryIter::new(&self.archetypes)
    }

    /// Query all entities mutably matching `Q`.
    pub fn query_mut<Q: QuerySpecMut>(&mut self) -> QueryIterMut<'_, Q> {
        let tick = self.change_tick;
        QueryIterMut::new(&mut self.archetypes, tick)
    }

    /// Query all entities mutably matching `Q` and `F`.
    pub fn query_filtered_mut<Q: QuerySpecMut, F: QueryFilter>(
        &mut self,
    ) -> QueryIterMut<'_, Q, F> {
        let tick = self.change_tick;
        QueryIterMut::new(&mut self.archetypes, tick)
    }

    /// Convenience wrapper for two-component immutable queries.
    pub fn query2<A: Component, B: Component>(&self) -> Query2Iter<'_, A, B> {
        self.query::<(&A, &B)>()
    }

    /// Convenience wrapper for two-component mutable queries.
    pub fn query2_mut<A: Component, B: Component>(&mut self) -> Query2MutIter<'_, A, B> {
        assert_ne!(
            TypeId::of::<A>(),
            TypeId::of::<B>(),
            "cannot borrow the same column mutably twice"
        );
        self.query_mut::<(&mut A, &mut B)>()
    }

    /// Convenience wrapper for three-component immutable queries.
    pub fn query3<A: Component, B: Component, C: Component>(&self) -> Query3Iter<'_, A, B, C> {
        self.query::<(&A, &B, &C)>()
    }

    /// Convenience wrapper for three-component mutable queries.
    pub fn query3_mut<A: Component, B: Component, C: Component>(
        &mut self,
    ) -> Query3MutIter<'_, A, B, C> {
        assert_ne!(
            TypeId::of::<A>(),
            TypeId::of::<B>(),
            "cannot borrow the same column mutably twice"
        );
        assert_ne!(
            TypeId::of::<A>(),
            TypeId::of::<C>(),
            "cannot borrow the same column mutably twice"
        );
        assert_ne!(
            TypeId::of::<B>(),
            TypeId::of::<C>(),
            "cannot borrow the same column mutably twice"
        );
        self.query_mut::<(&mut A, &mut B, &mut C)>()
    }

    // -------------------------------------------------------------------------
    // Change-detection queries
    // -------------------------------------------------------------------------

    /// Iterate entities whose component `T` changed after `since_tick`.
    pub fn query_changed<T: Component>(&self, since_tick: u64) -> ChangedIter<'_, T> {
        ChangedIter::new(&self.archetypes, since_tick)
    }

    /// Iterate entities whose component `T` was added after `since_tick`.
    pub fn query_added<T: Component>(&self, since_tick: u64) -> AddedIter<'_, T> {
        AddedIter::new(&self.archetypes, since_tick)
    }

    // -------------------------------------------------------------------------
    // Metadata
    // -------------------------------------------------------------------------

    /// Returns the number of alive entities.
    pub fn entity_count(&self) -> usize {
        self.meta.alive_entities().count()
    }

    /// Shared access to the archetype store (for change-detection inspection).
    pub fn archetypes(&self) -> &ArchetypeStore {
        &self.archetypes
    }

    /// Look up where an entity lives in archetype storage.
    pub fn entity_location(&self, entity: Entity) -> Option<EntityLocation> {
        self.meta.get_location(entity)
    }
}

impl Default for World {
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

    #[derive(Debug, Clone, PartialEq)]
    struct Pos {
        x: f32,
        y: f32,
    }
    impl Component for Pos {}

    #[derive(Debug, Clone, PartialEq)]
    struct Vel {
        x: f32,
        y: f32,
    }
    impl Component for Vel {}

    #[derive(Debug, Clone)]
    #[allow(dead_code)]
    struct Health(i32);
    impl Component for Health {}

    // -- Original tests (behavioral equivalence) --

    #[test]
    fn spawn_and_get() {
        let mut world = World::new();
        let e = world.spawn((Pos { x: 1.0, y: 2.0 },));
        assert!(world.is_alive(e));
        let pos = world.get::<Pos>(e).unwrap();
        assert_eq!(pos.x, 1.0);
        assert_eq!(pos.y, 2.0);
    }

    #[test]
    fn spawn_multi_component() {
        let mut world = World::new();
        let e = world.spawn((Pos { x: 0.0, y: 0.0 }, Vel { x: 1.0, y: 2.0 }));
        assert!(world.get::<Pos>(e).is_some());
        assert!(world.get::<Vel>(e).is_some());
    }

    #[test]
    fn despawn_removes_entity_and_components() {
        let mut world = World::new();
        let e = world.spawn((Pos { x: 0.0, y: 0.0 }, Health(100)));
        assert!(world.despawn(e));
        assert!(!world.is_alive(e));
        assert!(world.get::<Pos>(e).is_none());
        assert!(world.get::<Health>(e).is_none());
    }

    #[test]
    fn query_iterates_matching_entities() {
        let mut world = World::new();
        world.spawn((Pos { x: 1.0, y: 0.0 },));
        world.spawn((Pos { x: 2.0, y: 0.0 }, Vel { x: 0.0, y: 0.0 }));
        world.spawn((Vel { x: 3.0, y: 0.0 },)); // no Pos

        let positions: Vec<f32> = world.query::<&Pos>().map(|(_, p)| p.x).collect();
        assert_eq!(positions.len(), 2);
        assert!(positions.contains(&1.0));
        assert!(positions.contains(&2.0));
    }

    #[test]
    fn query_mut_allows_modification() {
        let mut world = World::new();
        world.spawn((Pos { x: 0.0, y: 0.0 },));
        world.spawn((Pos { x: 10.0, y: 10.0 },));

        for (_, pos) in world.query_mut::<&mut Pos>() {
            pos.x += 1.0;
        }

        let xs: Vec<f32> = world.query::<&Pos>().map(|(_, p)| p.x).collect();
        assert!(xs.contains(&1.0));
        assert!(xs.contains(&11.0));
    }

    #[test]
    fn resources() {
        let mut world = World::new();

        struct DeltaTime(f64);

        world.insert_resource(DeltaTime(0.016));
        assert_eq!(world.resource::<DeltaTime>().0, 0.016);

        world.resource_mut::<DeltaTime>().0 = 0.032;
        assert_eq!(world.resource::<DeltaTime>().0, 0.032);
    }

    #[test]
    fn entity_count() {
        let mut world = World::new();
        assert_eq!(world.entity_count(), 0);
        let e1 = world.spawn((Pos { x: 0.0, y: 0.0 },));
        let _e2 = world.spawn((Pos { x: 0.0, y: 0.0 },));
        assert_eq!(world.entity_count(), 2);
        world.despawn(e1);
        assert_eq!(world.entity_count(), 1);
    }

    #[test]
    fn query2_returns_entities_with_both_components() {
        let mut world = World::new();
        world.spawn((Pos { x: 1.0, y: 0.0 },));
        world.spawn((Pos { x: 2.0, y: 0.0 }, Vel { x: 5.0, y: 0.0 }));
        world.spawn((Vel { x: 3.0, y: 0.0 },));

        let results: Vec<_> = world.query::<(&Pos, &Vel)>().collect();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1.0.x, 2.0);
        assert_eq!(results[0].1.1.x, 5.0);
    }

    #[test]
    fn query2_mut_mutates_both_components() {
        let mut world = World::new();
        let e1 = world.spawn((Pos { x: 1.0, y: 1.0 }, Vel { x: 10.0, y: 10.0 }));
        let e2 = world.spawn((Pos { x: 2.0, y: 2.0 }, Vel { x: 20.0, y: 20.0 }));
        let e3 = world.spawn((Pos { x: 3.0, y: 3.0 }, Vel { x: 30.0, y: 30.0 }));

        for (_, (pos, vel)) in world.query_mut::<(&mut Pos, &mut Vel)>() {
            pos.x += 100.0;
            vel.y += 200.0;
        }

        assert_eq!(world.get::<Pos>(e1).unwrap().x, 101.0);
        assert_eq!(world.get::<Vel>(e1).unwrap().y, 210.0);
        assert_eq!(world.get::<Pos>(e2).unwrap().x, 102.0);
        assert_eq!(world.get::<Vel>(e2).unwrap().y, 220.0);
        assert_eq!(world.get::<Pos>(e3).unwrap().x, 103.0);
        assert_eq!(world.get::<Vel>(e3).unwrap().y, 230.0);
    }

    #[test]
    fn query2_mut_skips_entities_missing_one_component() {
        let mut world = World::new();
        let e1 = world.spawn((Pos { x: 5.0, y: 5.0 },));
        let e2 = world.spawn((Pos { x: 7.0, y: 7.0 }, Vel { x: 9.0, y: 9.0 }));
        let e3 = world.spawn((Vel { x: 11.0, y: 11.0 },));

        let results: Vec<_> = world.query_mut::<(&mut Pos, &mut Vel)>().collect();
        assert_eq!(results.len(), 1);

        let (entity, (pos, vel)) = &results[0];
        assert_eq!(*entity, e2);
        assert_eq!(pos.x, 7.0);
        assert_eq!(vel.x, 9.0);

        // e1's Pos must be unchanged.
        assert_eq!(world.get::<Pos>(e1).unwrap().x, 5.0);
        // e3's Vel must be unchanged.
        assert_eq!(world.get::<Vel>(e3).unwrap().x, 11.0);
    }

    #[test]
    #[should_panic(expected = "cannot borrow the same column mutably twice")]
    fn query2_mut_same_type_panics() {
        let mut world = World::new();
        world.spawn((Pos { x: 0.0, y: 0.0 },));
        let _ = world.query_mut::<(&mut Pos, &mut Pos)>().next();
    }

    #[test]
    fn query_filtered_with_and_without() {
        let mut world = World::new();
        let e1 = world.spawn((Pos { x: 1.0, y: 0.0 }, Vel { x: 2.0, y: 0.0 }));
        let _e2 = world.spawn((Pos { x: 3.0, y: 0.0 },));
        let _e3 = world.spawn((Pos { x: 4.0, y: 0.0 }, Health(100)));

        let results: Vec<_> = world
            .query_filtered::<&Pos, (crate::query::With<Vel>, crate::query::Without<Health>)>()
            .collect();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, e1);
        assert_eq!(results[0].1.x, 1.0);
    }

    #[test]
    fn one_and_one_mut_fetch_single_entity() {
        let mut world = World::new();
        let e = world.spawn((Pos { x: 1.0, y: 2.0 }, Vel { x: 3.0, y: 4.0 }));

        let (pos, vel) = world.one::<(&Pos, &Vel)>(e).unwrap();
        assert_eq!(pos.x, 1.0);
        assert_eq!(vel.x, 3.0);

        let (pos, vel) = world.one_mut::<(&mut Pos, &mut Vel)>(e).unwrap();
        pos.x = 10.0;
        vel.y = 20.0;

        assert_eq!(world.get::<Pos>(e).unwrap().x, 10.0);
        assert_eq!(world.get::<Vel>(e).unwrap().y, 20.0);
    }

    // -- New tests for insert/remove (archetype migration) --

    #[test]
    fn insert_adds_component_to_entity() {
        let mut world = World::new();
        let e = world.spawn((Pos { x: 1.0, y: 2.0 },));
        assert!(world.get::<Vel>(e).is_none());

        world.insert(e, Vel { x: 3.0, y: 4.0 });

        assert_eq!(world.get::<Vel>(e).unwrap().x, 3.0);
        // Original component preserved.
        assert_eq!(world.get::<Pos>(e).unwrap().x, 1.0);
    }

    #[test]
    fn insert_overwrites_existing_component() {
        let mut world = World::new();
        let e = world.spawn((Pos { x: 1.0, y: 2.0 },));
        world.insert(e, Pos { x: 99.0, y: 99.0 });
        assert_eq!(world.get::<Pos>(e).unwrap().x, 99.0);
    }

    #[test]
    fn insert_dead_entity_is_noop() {
        let mut world = World::new();
        let e = world.spawn((Pos { x: 1.0, y: 2.0 },));
        world.despawn(e);
        world.insert(e, Vel { x: 1.0, y: 1.0 }); // should not panic
    }

    #[test]
    fn insert_preserves_other_entities() {
        let mut world = World::new();
        let e1 = world.spawn((Pos { x: 1.0, y: 1.0 },));
        let e2 = world.spawn((Pos { x: 2.0, y: 2.0 },));

        world.insert(e1, Vel { x: 10.0, y: 10.0 });

        // e1 migrated, e2 stays in original archetype.
        assert_eq!(world.get::<Pos>(e1).unwrap().x, 1.0);
        assert_eq!(world.get::<Vel>(e1).unwrap().x, 10.0);
        assert_eq!(world.get::<Pos>(e2).unwrap().x, 2.0);
        assert!(world.get::<Vel>(e2).is_none());
    }

    #[test]
    fn remove_removes_component_from_entity() {
        let mut world = World::new();
        let e = world.spawn((Pos { x: 1.0, y: 2.0 }, Vel { x: 3.0, y: 4.0 }));
        world.remove::<Vel>(e);

        assert!(world.get::<Vel>(e).is_none());
        // Pos preserved.
        assert_eq!(world.get::<Pos>(e).unwrap().x, 1.0);
    }

    #[test]
    fn remove_absent_component_is_noop() {
        let mut world = World::new();
        let e = world.spawn((Pos { x: 1.0, y: 2.0 },));
        world.remove::<Vel>(e); // no-op
        assert_eq!(world.get::<Pos>(e).unwrap().x, 1.0);
    }

    #[test]
    fn remove_dead_entity_is_noop() {
        let mut world = World::new();
        let e = world.spawn((Pos { x: 1.0, y: 2.0 },));
        world.despawn(e);
        world.remove::<Pos>(e); // should not panic
    }

    #[test]
    fn remove_preserves_other_entities() {
        let mut world = World::new();
        let e1 = world.spawn((Pos { x: 1.0, y: 1.0 }, Vel { x: 10.0, y: 10.0 }));
        let e2 = world.spawn((Pos { x: 2.0, y: 2.0 }, Vel { x: 20.0, y: 20.0 }));

        world.remove::<Vel>(e1);

        assert!(world.get::<Vel>(e1).is_none());
        assert_eq!(world.get::<Pos>(e1).unwrap().x, 1.0);
        // e2 unaffected.
        assert_eq!(world.get::<Pos>(e2).unwrap().x, 2.0);
        assert_eq!(world.get::<Vel>(e2).unwrap().x, 20.0);
    }

    #[test]
    fn insert_then_query_finds_migrated_entity() {
        let mut world = World::new();
        let e1 = world.spawn((Pos { x: 1.0, y: 0.0 },));
        let _e2 = world.spawn((Pos { x: 2.0, y: 0.0 }, Vel { x: 20.0, y: 0.0 }));

        // e1 migrates from {Pos} to {Pos, Vel}
        world.insert(e1, Vel { x: 10.0, y: 0.0 });

        // Both entities now have Pos+Vel
        let results: Vec<_> = world.query::<(&Pos, &Vel)>().collect();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn remove_last_component_moves_to_empty_archetype() {
        let mut world = World::new();
        let e = world.spawn((Pos { x: 1.0, y: 2.0 },));
        world.remove::<Pos>(e);
        assert!(world.is_alive(e));
        assert!(world.get::<Pos>(e).is_none());
        // Entity is alive but has no components (empty archetype).
        assert_eq!(world.entity_count(), 1);
    }

    #[test]
    fn despawn_after_migration_cleans_up() {
        let mut world = World::new();
        let e = world.spawn((Pos { x: 1.0, y: 2.0 },));
        world.insert(e, Vel { x: 3.0, y: 4.0 });
        assert!(world.despawn(e));
        assert!(!world.is_alive(e));
        assert_eq!(world.entity_count(), 0);
    }

    #[test]
    fn spawn_duplicate_component_types_panics_without_mutating_world() {
        let mut world = World::new();

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            world.spawn((Pos { x: 1.0, y: 2.0 }, Pos { x: 3.0, y: 4.0 }));
        }));

        assert!(result.is_err());
        assert_eq!(world.entity_count(), 0);
        assert!(world.query::<&Pos>().next().is_none());
    }

    // ---- Change-detection tick -----------------------------------------------

    #[test]
    fn change_tick_starts_at_one() {
        let world = World::new();
        assert_eq!(world.change_tick(), 1);
    }

    #[test]
    fn advance_tick_increments() {
        let mut world = World::new();
        assert_eq!(world.change_tick(), 1);
        let t = world.advance_tick();
        assert_eq!(t, 2);
        assert_eq!(world.change_tick(), 2);
        world.advance_tick();
        assert_eq!(world.change_tick(), 3);
    }

    #[test]
    fn unsafe_world_cell_reads_change_tick() {
        let mut world = World::new();
        world.advance_tick();
        let cell = unsafe { UnsafeWorldCell::new(&mut world as *mut World) };
        assert_eq!(unsafe { cell.change_tick() }, 2);
    }

    // ---- Tick stamping on mutation -------------------------------------------

    #[test]
    fn spawn_stamps_added_and_changed_ticks() {
        let mut world = World::new();
        // change_tick starts at 1
        let e = world.spawn((Pos { x: 1.0, y: 2.0 },));
        let loc = world.entity_location(e).unwrap();
        let arch = world.archetypes().get(loc.archetype_id);
        let col = arch.column::<Pos>().unwrap();
        assert_eq!(col.added_tick(loc.row as usize), 1);
        assert_eq!(col.changed_tick(loc.row as usize), 1);
    }

    #[test]
    fn get_mut_stamps_changed_tick() {
        let mut world = World::new();
        let e = world.spawn((Pos { x: 1.0, y: 2.0 },));
        world.advance_tick(); // tick → 2
        world.get_mut::<Pos>(e).unwrap().x = 99.0;

        let loc = world.entity_location(e).unwrap();
        let arch = world.archetypes().get(loc.archetype_id);
        let col = arch.column::<Pos>().unwrap();
        assert_eq!(col.added_tick(loc.row as usize), 1); // unchanged
        assert_eq!(col.changed_tick(loc.row as usize), 2); // stamped
    }

    #[test]
    fn insert_overwrite_stamps_changed_tick() {
        let mut world = World::new();
        let e = world.spawn((Pos { x: 1.0, y: 2.0 },));
        world.advance_tick(); // tick → 2
        world.insert(e, Pos { x: 99.0, y: 0.0 });

        let loc = world.entity_location(e).unwrap();
        let arch = world.archetypes().get(loc.archetype_id);
        let col = arch.column::<Pos>().unwrap();
        assert_eq!(col.added_tick(loc.row as usize), 1);
        assert_eq!(col.changed_tick(loc.row as usize), 2);
    }

    #[test]
    fn insert_migration_stamps_new_component_ticks() {
        let mut world = World::new();
        let e = world.spawn((Pos { x: 1.0, y: 2.0 },));
        world.advance_tick(); // tick → 2
        world.insert(e, Vel { x: 3.0, y: 4.0 });

        let loc = world.entity_location(e).unwrap();
        let arch = world.archetypes().get(loc.archetype_id);

        // Pos was migrated — ticks preserved from spawn (tick 1).
        let pos_col = arch.column::<Pos>().unwrap();
        assert_eq!(pos_col.added_tick(loc.row as usize), 1);
        assert_eq!(pos_col.changed_tick(loc.row as usize), 1);

        // Vel is newly added at tick 2.
        let vel_col = arch.column::<Vel>().unwrap();
        assert_eq!(vel_col.added_tick(loc.row as usize), 2);
        assert_eq!(vel_col.changed_tick(loc.row as usize), 2);
    }

    #[test]
    fn query_mut_stamps_changed_tick() {
        let mut world = World::new();
        world.spawn((Pos { x: 1.0, y: 2.0 },));
        world.advance_tick(); // tick → 2
        for (_, pos) in world.query_mut::<&mut Pos>() {
            pos.x = 99.0;
        }

        // Check that changed_tick was stamped at tick 2.
        for (e, _) in world.query::<&Pos>() {
            let loc = world.entity_location(e).unwrap();
            let arch = world.archetypes().get(loc.archetype_id);
            let col = arch.column::<Pos>().unwrap();
            assert_eq!(col.changed_tick(loc.row as usize), 2);
        }
    }

    // ---- query_changed / query_added ----------------------------------------

    #[test]
    fn query_changed_returns_mutated_entities() {
        let mut world = World::new();
        let e1 = world.spawn((Pos { x: 1.0, y: 0.0 },));
        let e2 = world.spawn((Pos { x: 2.0, y: 0.0 },));
        let _e3 = world.spawn((Pos { x: 3.0, y: 0.0 },));

        let since = world.change_tick(); // 1
        world.advance_tick(); // tick → 2

        // Mutate only e1 and e2.
        world.get_mut::<Pos>(e1).unwrap().x = 10.0;
        world.get_mut::<Pos>(e2).unwrap().x = 20.0;

        let changed: Vec<Entity> = world.query_changed::<Pos>(since).map(|(e, _)| e).collect();
        assert_eq!(changed.len(), 2);
        assert!(changed.contains(&e1));
        assert!(changed.contains(&e2));
    }

    #[test]
    fn query_changed_excludes_untouched() {
        let mut world = World::new();
        world.spawn((Pos { x: 1.0, y: 0.0 },));

        let since = world.change_tick();
        world.advance_tick();
        // Don't mutate anything.

        let changed: Vec<Entity> = world.query_changed::<Pos>(since).map(|(e, _)| e).collect();
        assert!(changed.is_empty());
    }

    #[test]
    fn query_added_returns_newly_spawned() {
        let mut world = World::new();
        let _old = world.spawn((Pos { x: 1.0, y: 0.0 },));

        let since = world.change_tick(); // 1
        world.advance_tick(); // tick → 2

        let new = world.spawn((Pos { x: 2.0, y: 0.0 },));

        let added: Vec<Entity> = world.query_added::<Pos>(since).map(|(e, _)| e).collect();
        assert_eq!(added, vec![new]);
    }

    #[test]
    fn query_added_returns_component_inserted_via_migration() {
        let mut world = World::new();
        let e = world.spawn((Pos { x: 1.0, y: 0.0 },));

        let since = world.change_tick();
        world.advance_tick();

        world.insert(e, Vel { x: 3.0, y: 4.0 });

        // Vel was added after since_tick.
        let added: Vec<Entity> = world.query_added::<Vel>(since).map(|(e, _)| e).collect();
        assert_eq!(added, vec![e]);

        // Pos was added at tick 1, not after since_tick (also 1).
        let added_pos: Vec<Entity> = world.query_added::<Pos>(since).map(|(e, _)| e).collect();
        assert!(added_pos.is_empty());
    }

    #[test]
    fn query_changed_across_multiple_archetypes() {
        let mut world = World::new();
        // Archetype 1: (Pos,)
        let e1 = world.spawn((Pos { x: 1.0, y: 0.0 },));
        // Archetype 2: (Pos, Vel)
        let e2 = world.spawn((Pos { x: 2.0, y: 0.0 }, Vel { x: 0.0, y: 0.0 }));

        let since = world.change_tick();
        world.advance_tick();

        // Mutate Pos on both.
        world.get_mut::<Pos>(e1).unwrap().x = 10.0;
        world.get_mut::<Pos>(e2).unwrap().x = 20.0;

        let changed: Vec<Entity> = world.query_changed::<Pos>(since).map(|(e, _)| e).collect();
        assert_eq!(changed.len(), 2);
        assert!(changed.contains(&e1));
        assert!(changed.contains(&e2));
    }
}
