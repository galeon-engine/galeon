// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

use std::any::TypeId;
use std::marker::PhantomData;

use crate::archetype::{Archetype, ArchetypeLayout, ArchetypeStore, Column};
use crate::component::Component;
use crate::entity::Entity;

/// Describes an immutable query fetch over matching archetypes.
pub trait QuerySpec {
    type Item<'w>;

    type State<'w>;

    fn matches(layout: &ArchetypeLayout) -> bool;

    fn init_state<'w>(archetype: &'w Archetype) -> Option<Self::State<'w>>;

    fn len(state: &Self::State<'_>) -> usize;

    fn entity(state: &Self::State<'_>, row: usize) -> Entity;

    fn fetch<'w>(state: &Self::State<'w>, row: usize) -> Self::Item<'w>;
}

/// Describes a mutable query fetch over matching archetypes.
pub trait QuerySpecMut {
    type Item<'w>;

    type State<'w>;

    fn matches(layout: &ArchetypeLayout) -> bool;

    fn init_state<'w>(archetype: &'w mut Archetype, tick: u64) -> Option<Self::State<'w>>;

    fn len(state: &Self::State<'_>) -> usize;

    fn entity(state: &Self::State<'_>, row: usize) -> Entity;

    /// # Safety
    ///
    /// Callers must only request each row at most once per live state. Doing
    /// otherwise could create aliased mutable references into the same
    /// archetype column data.
    unsafe fn fetch<'w>(state: &mut Self::State<'w>, row: usize) -> Self::Item<'w>;

    /// Stamp `changed_tick` on all mutable columns touched by this query at
    /// `row`. Called by `QueryIterMut::next()` after `fetch()`.
    ///
    /// # Safety
    ///
    /// The `changed_ticks` pointers in `state` must be valid for writes at `row`.
    unsafe fn stamp_changed(state: &mut Self::State<'_>, row: usize);
}

/// Restricts which archetypes participate in a query.
pub trait QueryFilter {
    fn matches(layout: &ArchetypeLayout) -> bool;
}

/// No-op filter used by plain `query()` / `query_mut()`.
pub struct NoFilter;

impl QueryFilter for NoFilter {
    fn matches(_layout: &ArchetypeLayout) -> bool {
        true
    }
}

/// Matches archetypes that contain component `T`.
pub struct With<T>(PhantomData<T>);

impl<T: Component> QueryFilter for With<T> {
    fn matches(layout: &ArchetypeLayout) -> bool {
        layout.contains(TypeId::of::<T>())
    }
}

/// Matches archetypes that do not contain component `T`.
pub struct Without<T>(PhantomData<T>);

impl<T: Component> QueryFilter for Without<T> {
    fn matches(layout: &ArchetypeLayout) -> bool {
        !layout.contains(TypeId::of::<T>())
    }
}

impl<A, B> QueryFilter for (A, B)
where
    A: QueryFilter,
    B: QueryFilter,
{
    fn matches(layout: &ArchetypeLayout) -> bool {
        A::matches(layout) && B::matches(layout)
    }
}

impl<A, B, C> QueryFilter for (A, B, C)
where
    A: QueryFilter,
    B: QueryFilter,
    C: QueryFilter,
{
    fn matches(layout: &ArchetypeLayout) -> bool {
        A::matches(layout) && B::matches(layout) && C::matches(layout)
    }
}

/// Zero-allocation immutable archetype query iterator.
pub struct QueryIter<'w, Q, F = NoFilter>
where
    Q: QuerySpec,
    F: QueryFilter,
{
    store: &'w ArchetypeStore,
    archetype_index: usize,
    row: usize,
    current: Option<Q::State<'w>>,
    _filter: PhantomData<F>,
}

impl<'w, Q, F> QueryIter<'w, Q, F>
where
    Q: QuerySpec,
    F: QueryFilter,
{
    pub(crate) fn new(store: &'w ArchetypeStore) -> Self {
        Self {
            store,
            archetype_index: 0,
            row: 0,
            current: None,
            _filter: PhantomData,
        }
    }

    fn remaining(&self) -> usize {
        let current = self
            .current
            .as_ref()
            .map_or(0, |state| Q::len(state) - self.row);
        let future: usize = self
            .store
            .iter()
            .skip(self.archetype_index)
            .filter(|archetype| Q::matches(archetype.layout()) && F::matches(archetype.layout()))
            .map(|archetype| archetype.len())
            .sum();
        current + future
    }
}

impl<'w, Q, F> Iterator for QueryIter<'w, Q, F>
where
    Q: QuerySpec + 'w,
    F: QueryFilter,
{
    type Item = (Entity, Q::Item<'w>);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(state) = self.current.as_ref() {
                if self.row < Q::len(state) {
                    let row = self.row;
                    self.row += 1;
                    return Some((Q::entity(state, row), Q::fetch(state, row)));
                }
                self.current = None;
            }

            let archetype = self.store.get_by_index(self.archetype_index)?;
            self.archetype_index += 1;

            if !Q::matches(archetype.layout()) || !F::matches(archetype.layout()) {
                continue;
            }

            self.current = Q::init_state(archetype);
            self.row = 0;
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.remaining();
        (remaining, Some(remaining))
    }
}

impl<'w, Q, F> ExactSizeIterator for QueryIter<'w, Q, F>
where
    Q: QuerySpec + 'w,
    F: QueryFilter,
{
    fn len(&self) -> usize {
        self.remaining()
    }
}

/// Zero-allocation mutable archetype query iterator.
pub struct QueryIterMut<'w, Q, F = NoFilter>
where
    Q: QuerySpecMut,
    F: QueryFilter,
{
    store: *mut ArchetypeStore,
    archetype_len: usize,
    archetype_index: usize,
    row: usize,
    tick: u64,
    current: Option<Q::State<'w>>,
    _filter: PhantomData<F>,
    _marker: PhantomData<&'w mut ArchetypeStore>,
}

impl<'w, Q, F> QueryIterMut<'w, Q, F>
where
    Q: QuerySpecMut,
    F: QueryFilter,
{
    pub(crate) fn new(store: &'w mut ArchetypeStore, tick: u64) -> Self {
        Self {
            archetype_len: store.len(),
            store,
            archetype_index: 0,
            row: 0,
            tick,
            current: None,
            _filter: PhantomData,
            _marker: PhantomData,
        }
    }

    /// Construct from a raw pointer without creating `&mut ArchetypeStore`.
    ///
    /// This is the `UnsafeWorldCell` path: avoids the intermediate
    /// `&mut ArchetypeStore` that `new()` requires, eliminating the
    /// `&ArchetypeStore` / `&mut ArchetypeStore` overlap when `Query<A>`
    /// and `QueryMut<B>` are fetched concurrently.
    ///
    /// # Safety
    ///
    /// - `store` must be a valid, non-null pointer to an `ArchetypeStore`
    ///   that lives for `'w`.
    /// - The caller must guarantee exclusive mutable access to the columns
    ///   that `Q` touches (enforced by conflict detection).
    pub(crate) unsafe fn new_from_ptr(store: *mut ArchetypeStore, tick: u64) -> Self {
        Self {
            archetype_len: unsafe { (*store).len() },
            store,
            archetype_index: 0,
            row: 0,
            tick,
            current: None,
            _filter: PhantomData,
            _marker: PhantomData,
        }
    }

    fn remaining(&self) -> usize {
        let current = self
            .current
            .as_ref()
            .map_or(0, |state| Q::len(state) - self.row);
        // SAFETY: The iterator owns the mutable store borrow for its entire
        // lifetime, and this helper only reads future archetype metadata.
        let future: usize = unsafe { &*self.store }
            .iter()
            .skip(self.archetype_index)
            .filter(|archetype| Q::matches(archetype.layout()) && F::matches(archetype.layout()))
            .map(|archetype| archetype.len())
            .sum();
        current + future
    }
}

impl<'w, Q, F> Iterator for QueryIterMut<'w, Q, F>
where
    Q: QuerySpecMut + 'w,
    F: QueryFilter,
{
    type Item = (Entity, Q::Item<'w>);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(state) = self.current.as_mut() {
                if self.row < Q::len(state) {
                    let row = self.row;
                    self.row += 1;
                    let entity = Q::entity(state, row);
                    // SAFETY: `QueryIterMut` only advances forward through a
                    // single archetype state and never yields the same row
                    // twice, so mutable references produced for one row cannot
                    // alias later yields from the same state.
                    let item = unsafe { Q::fetch(state, row) };
                    // SAFETY: changed_ticks pointers in state are valid for
                    // this row (same column layout guarantees as data pointers).
                    unsafe { Q::stamp_changed(state, row) };
                    return Some((entity, item));
                }
                self.current = None;
            }

            if self.archetype_index >= self.archetype_len {
                return None;
            }

            // SAFETY: Uses `get_by_index_mut_ptr` which reaches the
            // `archetypes` Vec via `addr_of_mut!` — no `&mut ArchetypeStore`
            // is created, only `&mut Vec<Archetype>` at the field level.
            // This prevents Stacked Borrows invalidation of any concurrent
            // `&ArchetypeStore` borrows (e.g., from a `Query<A>` that was
            // fetched before this `QueryMut<B>`).
            //
            // `current` is cleared before borrowing a new archetype, so no
            // mutable state references the archetype being re-borrowed.
            // Items yielded from prior archetypes carry `&'w mut T` into
            // the caller, but those point into distinct per-archetype
            // `Column<T>` heap allocations — different archetypes own
            // separate column `Vec`s, so references from archetype A never
            // alias data in archetype B.
            let archetype =
                unsafe { ArchetypeStore::get_by_index_mut_ptr(self.store, self.archetype_index)? };
            self.archetype_index += 1;

            if !Q::matches(archetype.layout()) || !F::matches(archetype.layout()) {
                continue;
            }

            self.current = Q::init_state(archetype, self.tick);
            self.row = 0;
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.remaining();
        (remaining, Some(remaining))
    }
}

impl<'w, Q, F> ExactSizeIterator for QueryIterMut<'w, Q, F>
where
    Q: QuerySpecMut + 'w,
    F: QueryFilter,
{
    fn len(&self) -> usize {
        self.remaining()
    }
}

pub type Query2Iter<'w, A, B, F = NoFilter> = QueryIter<'w, (&'w A, &'w B), F>;
pub type Query2MutIter<'w, A, B, F = NoFilter> = QueryIterMut<'w, (&'w mut A, &'w mut B), F>;
pub type Query3Iter<'w, A, B, C, F = NoFilter> = QueryIter<'w, (&'w A, &'w B, &'w C), F>;
pub type Query3MutIter<'w, A, B, C, F = NoFilter> =
    QueryIterMut<'w, (&'w mut A, &'w mut B, &'w mut C), F>;

#[doc(hidden)]
pub struct OptionalMutState<'w, T> {
    entities: &'w [Entity],
    /// Null when the column is absent from the archetype.
    data: *mut T,
    _marker: PhantomData<&'w mut T>,
}

#[doc(hidden)]
pub struct SingleMutState<'w, T> {
    entities: &'w [Entity],
    data: *mut T,
    changed_ticks: *mut u64,
    tick: u64,
    len: usize,
    _marker: PhantomData<&'w mut T>,
}

#[doc(hidden)]
pub struct PairMutState<'w, A, B> {
    entities: &'w [Entity],
    col_a: *mut A,
    col_b: *mut B,
    changed_ticks_a: *mut u64,
    changed_ticks_b: *mut u64,
    tick: u64,
    len: usize,
    _marker: PhantomData<&'w mut (A, B)>,
}

#[doc(hidden)]
pub struct TripleMutState<'w, A, B, C> {
    entities: &'w [Entity],
    col_a: *mut A,
    col_b: *mut B,
    col_c: *mut C,
    changed_ticks_a: *mut u64,
    changed_ticks_b: *mut u64,
    changed_ticks_c: *mut u64,
    tick: u64,
    len: usize,
    _marker: PhantomData<&'w mut (A, B, C)>,
}

impl<T: Component> QuerySpec for &T {
    type Item<'w> = &'w T;

    type State<'w> = (&'w [Entity], &'w Column<T>);

    fn matches(layout: &ArchetypeLayout) -> bool {
        layout.contains(TypeId::of::<T>())
    }

    fn init_state<'w>(archetype: &'w Archetype) -> Option<Self::State<'w>> {
        Some((archetype.entities(), archetype.column::<T>()?))
    }

    fn len(state: &Self::State<'_>) -> usize {
        state.0.len()
    }

    fn entity(state: &Self::State<'_>, row: usize) -> Entity {
        state.0[row]
    }

    fn fetch<'w>(state: &Self::State<'w>, row: usize) -> Self::Item<'w> {
        state.1.get(row).unwrap()
    }
}

/// Optional immutable query: matches all archetypes (never filters), returns
/// `Some(&T)` when the column is present and `None` when absent.
impl<T: Component> QuerySpec for Option<&T> {
    type Item<'w> = Option<&'w T>;

    type State<'w> = (&'w [Entity], Option<&'w Column<T>>);

    fn matches(_layout: &ArchetypeLayout) -> bool {
        true
    }

    fn init_state<'w>(archetype: &'w Archetype) -> Option<Self::State<'w>> {
        Some((archetype.entities(), archetype.column::<T>()))
    }

    fn len(state: &Self::State<'_>) -> usize {
        state.0.len()
    }

    fn entity(state: &Self::State<'_>, row: usize) -> Entity {
        state.0[row]
    }

    fn fetch<'w>(state: &Self::State<'w>, row: usize) -> Self::Item<'w> {
        state.1.as_ref().and_then(|col| col.get(row))
    }
}

impl<A: Component, B: Component> QuerySpec for (&A, &B) {
    type Item<'w> = (&'w A, &'w B);

    type State<'w> = (&'w [Entity], &'w Column<A>, &'w Column<B>);

    fn matches(layout: &ArchetypeLayout) -> bool {
        layout.contains(TypeId::of::<A>()) && layout.contains(TypeId::of::<B>())
    }

    fn init_state<'w>(archetype: &'w Archetype) -> Option<Self::State<'w>> {
        Some((
            archetype.entities(),
            archetype.column::<A>()?,
            archetype.column::<B>()?,
        ))
    }

    fn len(state: &Self::State<'_>) -> usize {
        state.0.len()
    }

    fn entity(state: &Self::State<'_>, row: usize) -> Entity {
        state.0[row]
    }

    fn fetch<'w>(state: &Self::State<'w>, row: usize) -> Self::Item<'w> {
        (state.1.get(row).unwrap(), state.2.get(row).unwrap())
    }
}

impl<A: Component, B: Component> QuerySpec for (&A, Option<&B>) {
    type Item<'w> = (&'w A, Option<&'w B>);

    type State<'w> = (&'w [Entity], &'w Column<A>, Option<&'w Column<B>>);

    fn matches(layout: &ArchetypeLayout) -> bool {
        layout.contains(TypeId::of::<A>())
    }

    fn init_state<'w>(archetype: &'w Archetype) -> Option<Self::State<'w>> {
        Some((
            archetype.entities(),
            archetype.column::<A>()?,
            archetype.column::<B>(),
        ))
    }

    fn len(state: &Self::State<'_>) -> usize {
        state.0.len()
    }

    fn entity(state: &Self::State<'_>, row: usize) -> Entity {
        state.0[row]
    }

    fn fetch<'w>(state: &Self::State<'w>, row: usize) -> Self::Item<'w> {
        (
            state.1.get(row).unwrap(),
            state.2.as_ref().and_then(|col| col.get(row)),
        )
    }
}

impl<A: Component, B: Component, C: Component> QuerySpec for (&A, &B, &C) {
    type Item<'w> = (&'w A, &'w B, &'w C);

    type State<'w> = (&'w [Entity], &'w Column<A>, &'w Column<B>, &'w Column<C>);

    fn matches(layout: &ArchetypeLayout) -> bool {
        layout.contains(TypeId::of::<A>())
            && layout.contains(TypeId::of::<B>())
            && layout.contains(TypeId::of::<C>())
    }

    fn init_state<'w>(archetype: &'w Archetype) -> Option<Self::State<'w>> {
        Some((
            archetype.entities(),
            archetype.column::<A>()?,
            archetype.column::<B>()?,
            archetype.column::<C>()?,
        ))
    }

    fn len(state: &Self::State<'_>) -> usize {
        state.0.len()
    }

    fn entity(state: &Self::State<'_>, row: usize) -> Entity {
        state.0[row]
    }

    fn fetch<'w>(state: &Self::State<'w>, row: usize) -> Self::Item<'w> {
        (
            state.1.get(row).unwrap(),
            state.2.get(row).unwrap(),
            state.3.get(row).unwrap(),
        )
    }
}

impl<A: Component, B: Component, C: Component, D: Component> QuerySpec
    for (&A, Option<&B>, Option<&C>, Option<&D>)
{
    type Item<'w> = (&'w A, Option<&'w B>, Option<&'w C>, Option<&'w D>);

    type State<'w> = (
        &'w [Entity],
        &'w Column<A>,
        Option<&'w Column<B>>,
        Option<&'w Column<C>>,
        Option<&'w Column<D>>,
    );

    fn matches(layout: &ArchetypeLayout) -> bool {
        layout.contains(TypeId::of::<A>())
    }

    fn init_state<'w>(archetype: &'w Archetype) -> Option<Self::State<'w>> {
        Some((
            archetype.entities(),
            archetype.column::<A>()?,
            archetype.column::<B>(),
            archetype.column::<C>(),
            archetype.column::<D>(),
        ))
    }

    fn len(state: &Self::State<'_>) -> usize {
        state.0.len()
    }

    fn entity(state: &Self::State<'_>, row: usize) -> Entity {
        state.0[row]
    }

    fn fetch<'w>(state: &Self::State<'w>, row: usize) -> Self::Item<'w> {
        (
            state.1.get(row).unwrap(),
            state.2.as_ref().and_then(|col| col.get(row)),
            state.3.as_ref().and_then(|col| col.get(row)),
            state.4.as_ref().and_then(|col| col.get(row)),
        )
    }
}

impl<T: Component> QuerySpecMut for &mut T {
    type Item<'w> = &'w mut T;

    type State<'w> = SingleMutState<'w, T>;

    fn matches(layout: &ArchetypeLayout) -> bool {
        layout.contains(TypeId::of::<T>())
    }

    fn init_state<'w>(archetype: &'w mut Archetype, tick: u64) -> Option<Self::State<'w>> {
        let (entities, column) = archetype.entities_and_column_mut::<T>()?;
        Some(SingleMutState {
            entities,
            data: column.as_mut_ptr(),
            changed_ticks: column.changed_ticks_mut_ptr(),
            tick,
            len: column.len(),
            _marker: PhantomData,
        })
    }

    fn len(state: &Self::State<'_>) -> usize {
        state.len
    }

    fn entity(state: &Self::State<'_>, row: usize) -> Entity {
        state.entities[row]
    }

    unsafe fn fetch<'w>(state: &mut Self::State<'w>, row: usize) -> Self::Item<'w> {
        // SAFETY: `QueryIterMut` guarantees that each row is yielded at most
        // once, so the mutable reference to this slot cannot alias another
        // live reference produced by the same iterator.
        unsafe { &mut *state.data.add(row) }
    }

    unsafe fn stamp_changed(state: &mut Self::State<'_>, row: usize) {
        unsafe { *state.changed_ticks.add(row) = state.tick }
    }
}

/// Optional mutable query: matches all archetypes (never filters), returns
/// `Some(&mut T)` when the column is present and `None` when absent.
impl<T: Component> QuerySpecMut for Option<&mut T> {
    type Item<'w> = Option<&'w mut T>;

    type State<'w> = OptionalMutState<'w, T>;

    fn matches(_layout: &ArchetypeLayout) -> bool {
        true
    }

    fn init_state<'w>(archetype: &'w mut Archetype) -> Option<Self::State<'w>> {
        let entities = archetype.entities() as *const [Entity];
        let data = match archetype.column_mut::<T>() {
            Some(col) => col.as_mut_ptr(),
            None => std::ptr::null_mut(),
        };
        // SAFETY: `entities` pointer is valid for the lifetime of the archetype
        // borrow, which is `'w`.
        Some(OptionalMutState {
            entities: unsafe { &*entities },
            data,
            _marker: PhantomData,
        })
    }

    fn len(state: &Self::State<'_>) -> usize {
        state.entities.len()
    }

    fn entity(state: &Self::State<'_>, row: usize) -> Entity {
        state.entities[row]
    }

    unsafe fn fetch<'w>(state: &mut Self::State<'w>, row: usize) -> Self::Item<'w> {
        if state.data.is_null() {
            None
        } else {
            // SAFETY: `QueryIterMut` guarantees each row is yielded at most once.
            Some(unsafe { &mut *state.data.add(row) })
        }
    }
}

impl<A: Component, B: Component> QuerySpecMut for (&mut A, &mut B) {
    type Item<'w> = (&'w mut A, &'w mut B);

    type State<'w> = PairMutState<'w, A, B>;

    fn matches(layout: &ArchetypeLayout) -> bool {
        layout.contains(TypeId::of::<A>()) && layout.contains(TypeId::of::<B>())
    }

    fn init_state<'w>(archetype: &'w mut Archetype, tick: u64) -> Option<Self::State<'w>> {
        let (entities, col_a, col_b) = archetype.entities_and_two_columns_mut::<A, B>()?;
        Some(PairMutState {
            entities,
            col_a: col_a.as_mut_ptr(),
            col_b: col_b.as_mut_ptr(),
            changed_ticks_a: col_a.changed_ticks_mut_ptr(),
            changed_ticks_b: col_b.changed_ticks_mut_ptr(),
            tick,
            len: entities.len(),
            _marker: PhantomData,
        })
    }

    fn len(state: &Self::State<'_>) -> usize {
        state.len
    }

    fn entity(state: &Self::State<'_>, row: usize) -> Entity {
        state.entities[row]
    }

    unsafe fn fetch<'w>(state: &mut Self::State<'w>, row: usize) -> Self::Item<'w> {
        // SAFETY: `entities_and_two_columns_mut` guarantees distinct columns
        // and `QueryIterMut` guarantees each row is yielded once.
        unsafe { (&mut *state.col_a.add(row), &mut *state.col_b.add(row)) }
    }

    unsafe fn stamp_changed(state: &mut Self::State<'_>, row: usize) {
        unsafe {
            *state.changed_ticks_a.add(row) = state.tick;
            *state.changed_ticks_b.add(row) = state.tick;
        }
    }
}

impl<A: Component, B: Component, C: Component> QuerySpecMut for (&mut A, &mut B, &mut C) {
    type Item<'w> = (&'w mut A, &'w mut B, &'w mut C);

    type State<'w> = TripleMutState<'w, A, B, C>;

    fn matches(layout: &ArchetypeLayout) -> bool {
        layout.contains(TypeId::of::<A>())
            && layout.contains(TypeId::of::<B>())
            && layout.contains(TypeId::of::<C>())
    }

    fn init_state<'w>(archetype: &'w mut Archetype, tick: u64) -> Option<Self::State<'w>> {
        let (entities, col_a, col_b, col_c) =
            archetype.entities_and_three_columns_mut::<A, B, C>()?;
        Some(TripleMutState {
            entities,
            col_a: col_a.as_mut_ptr(),
            col_b: col_b.as_mut_ptr(),
            col_c: col_c.as_mut_ptr(),
            changed_ticks_a: col_a.changed_ticks_mut_ptr(),
            changed_ticks_b: col_b.changed_ticks_mut_ptr(),
            changed_ticks_c: col_c.changed_ticks_mut_ptr(),
            tick,
            len: entities.len(),
            _marker: PhantomData,
        })
    }

    fn len(state: &Self::State<'_>) -> usize {
        state.len
    }

    fn entity(state: &Self::State<'_>, row: usize) -> Entity {
        state.entities[row]
    }

    unsafe fn fetch<'w>(state: &mut Self::State<'w>, row: usize) -> Self::Item<'w> {
        // SAFETY: `entities_and_three_columns_mut` guarantees three distinct
        // columns and `QueryIterMut` yields each row at most once.
        unsafe {
            (
                &mut *state.col_a.add(row),
                &mut *state.col_b.add(row),
                &mut *state.col_c.add(row),
            )
        }
    }

    unsafe fn stamp_changed(state: &mut Self::State<'_>, row: usize) {
        unsafe {
            *state.changed_ticks_a.add(row) = state.tick;
            *state.changed_ticks_b.add(row) = state.tick;
            *state.changed_ticks_c.add(row) = state.tick;
        }
    }
}

// =============================================================================
// Change-detection iterators
// =============================================================================

/// Iterator yielding entities whose component `T` has a `changed_tick > since_tick`.
pub struct ChangedIter<'w, T: Component> {
    store: &'w ArchetypeStore,
    archetype_index: usize,
    row: usize,
    since_tick: u64,
    current: Option<(&'w [Entity], &'w Column<T>)>,
}

impl<'w, T: Component> ChangedIter<'w, T> {
    pub(crate) fn new(store: &'w ArchetypeStore, since_tick: u64) -> Self {
        Self {
            store,
            archetype_index: 0,
            row: 0,
            since_tick,
            current: None,
        }
    }
}

impl<'w, T: Component> Iterator for ChangedIter<'w, T> {
    type Item = (Entity, &'w T);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some((entities, col)) = &self.current {
                while self.row < entities.len() {
                    let row = self.row;
                    self.row += 1;
                    if col.changed_tick(row) > self.since_tick {
                        return Some((entities[row], col.get(row).unwrap()));
                    }
                }
                self.current = None;
            }

            let archetype = self.store.get_by_index(self.archetype_index)?;
            self.archetype_index += 1;

            if !archetype.layout().contains(TypeId::of::<T>()) {
                continue;
            }

            if let (Some(col), entities) = (archetype.column::<T>(), archetype.entities()) {
                self.current = Some((entities, col));
                self.row = 0;
            }
        }
    }
}

/// Iterator yielding entities whose component `T` has an `added_tick > since_tick`.
pub struct AddedIter<'w, T: Component> {
    store: &'w ArchetypeStore,
    archetype_index: usize,
    row: usize,
    since_tick: u64,
    current: Option<(&'w [Entity], &'w Column<T>)>,
}

impl<'w, T: Component> AddedIter<'w, T> {
    pub(crate) fn new(store: &'w ArchetypeStore, since_tick: u64) -> Self {
        Self {
            store,
            archetype_index: 0,
            row: 0,
            since_tick,
            current: None,
        }
    }
}

impl<'w, T: Component> Iterator for AddedIter<'w, T> {
    type Item = (Entity, &'w T);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some((entities, col)) = &self.current {
                while self.row < entities.len() {
                    let row = self.row;
                    self.row += 1;
                    if col.added_tick(row) > self.since_tick {
                        return Some((entities[row], col.get(row).unwrap()));
                    }
                }
                self.current = None;
            }

            let archetype = self.store.get_by_index(self.archetype_index)?;
            self.archetype_index += 1;

            if !archetype.layout().contains(TypeId::of::<T>()) {
                continue;
            }

            if let (Some(col), entities) = (archetype.column::<T>(), archetype.entities()) {
                self.current = Some((entities, col));
                self.row = 0;
            }
        }
    }
}

#[doc(hidden)]
pub struct PairMutOptionalState<'w, A, B> {
    entities: &'w [Entity],
    col_a: *mut A,
    col_b: *mut B,
    len: usize,
    _marker: PhantomData<&'w mut (A, B)>,
}

impl<A: Component, B: Component> QuerySpecMut for (&mut A, Option<&mut B>) {
    type Item<'w> = (&'w mut A, Option<&'w mut B>);

    type State<'w> = PairMutOptionalState<'w, A, B>;

    fn matches(layout: &ArchetypeLayout) -> bool {
        layout.contains(TypeId::of::<A>())
    }

    fn init_state<'w>(archetype: &'w mut Archetype) -> Option<Self::State<'w>> {
        let entities = archetype.entities() as *const [Entity];
        // Required column A
        let col_a = archetype.column_mut::<A>()?;
        let col_a_ptr = col_a.as_mut_ptr();
        let len = col_a.len();
        // Optional column B (may be null)
        let col_b_ptr = archetype
            .column_mut::<B>()
            .map_or(std::ptr::null_mut(), |c| c.as_mut_ptr());
        // SAFETY: `entities` pointer is valid for `'w`.
        Some(PairMutOptionalState {
            entities: unsafe { &*entities },
            col_a: col_a_ptr,
            col_b: col_b_ptr,
            len,
            _marker: PhantomData,
        })
    }

    fn len(state: &Self::State<'_>) -> usize {
        state.len
    }

    fn entity(state: &Self::State<'_>, row: usize) -> Entity {
        state.entities[row]
    }

    unsafe fn fetch<'w>(state: &mut Self::State<'w>, row: usize) -> Self::Item<'w> {
        // SAFETY: `QueryIterMut` guarantees each row is yielded at most once.
        // A and B are distinct types, so column pointers cannot alias.
        unsafe {
            let a = &mut *state.col_a.add(row);
            let b = if state.col_b.is_null() {
                None
            } else {
                Some(&mut *state.col_b.add(row))
            };
            (a, b)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::World;

    #[derive(Debug, Clone, PartialEq)]
    struct Pos {
        x: f32,
        y: f32,
    }
    impl crate::Component for Pos {}

    #[derive(Debug, Clone, PartialEq)]
    struct Vel {
        x: f32,
        y: f32,
    }
    impl crate::Component for Vel {}

    #[derive(Debug, Clone, PartialEq)]
    struct Name(String);
    impl crate::Component for Name {}

    #[derive(Debug, Clone, PartialEq)]
    struct Health(i32);
    impl crate::Component for Health {}

    // -- Option<&T> standalone --

    #[test]
    fn optional_query_returns_some_when_present() {
        let mut world = World::new();
        world.spawn((Pos { x: 1.0, y: 2.0 },));

        let results: Vec<_> = world.query::<Option<&Pos>>().collect();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1, Some(&Pos { x: 1.0, y: 2.0 }));
    }

    #[test]
    fn optional_query_returns_none_when_absent() {
        let mut world = World::new();
        world.spawn((Vel { x: 1.0, y: 0.0 },));

        let results: Vec<_> = world.query::<Option<&Pos>>().collect();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1, None);
    }

    // -- (&A, Option<&B>) tuple --

    #[test]
    fn required_plus_optional_both_present() {
        let mut world = World::new();
        world.spawn((Pos { x: 1.0, y: 0.0 }, Vel { x: 2.0, y: 0.0 }));

        let results: Vec<_> = world.query::<(&Pos, Option<&Vel>)>().collect();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1.0, &Pos { x: 1.0, y: 0.0 });
        assert_eq!(results[0].1.1, Some(&Vel { x: 2.0, y: 0.0 }));
    }

    #[test]
    fn required_plus_optional_absent() {
        let mut world = World::new();
        world.spawn((Pos { x: 1.0, y: 0.0 },));

        let results: Vec<_> = world.query::<(&Pos, Option<&Vel>)>().collect();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1.1, None);
    }

    #[test]
    fn required_plus_optional_filters_by_required() {
        let mut world = World::new();
        world.spawn((Pos { x: 1.0, y: 0.0 },));
        world.spawn((Vel { x: 2.0, y: 0.0 },)); // no Pos — should NOT appear

        let results: Vec<_> = world.query::<(&Pos, Option<&Vel>)>().collect();
        assert_eq!(results.len(), 1);
    }

    // -- (&A, Option<&B>, Option<&C>, Option<&D>) 4-tuple --

    #[test]
    fn four_tuple_all_present() {
        let mut world = World::new();
        world.spawn((
            Pos { x: 1.0, y: 0.0 },
            Vel { x: 2.0, y: 0.0 },
            Name("a".into()),
            Health(100),
        ));

        let results: Vec<_> = world
            .query::<(&Pos, Option<&Vel>, Option<&Name>, Option<&Health>)>()
            .collect();
        assert_eq!(results.len(), 1);
        let (pos, vel, name, hp) = &results[0].1;
        assert_eq!(*pos, &Pos { x: 1.0, y: 0.0 });
        assert!(vel.is_some());
        assert!(name.is_some());
        assert!(hp.is_some());
    }

    #[test]
    fn four_tuple_mixed_presence() {
        let mut world = World::new();
        // Entity with Pos + Health only
        world.spawn((Pos { x: 1.0, y: 0.0 }, Health(50)));
        // Entity with Pos + Vel + Name only
        world.spawn((
            Pos { x: 2.0, y: 0.0 },
            Vel { x: 1.0, y: 0.0 },
            Name("b".into()),
        ));

        let mut results: Vec<_> = world
            .query::<(&Pos, Option<&Vel>, Option<&Name>, Option<&Health>)>()
            .map(|(_, (pos, vel, name, hp))| (pos.x, vel.is_some(), name.is_some(), hp.is_some()))
            .collect();
        results.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());

        assert_eq!(results.len(), 2);
        assert_eq!(results[0], (1.0, false, false, true));
        assert_eq!(results[1], (2.0, true, true, false));
    }

    // -- Option<&mut T> --

    #[test]
    fn optional_mut_modifies_when_present() {
        let mut world = World::new();
        world.spawn((Pos { x: 1.0, y: 0.0 }, Vel { x: 0.0, y: 0.0 }));

        for (_, vel) in world.query_mut::<Option<&mut Vel>>() {
            if let Some(v) = vel {
                v.x += 10.0;
            }
        }

        let vel = world.query::<&Vel>().next().unwrap().1;
        assert_eq!(vel.x, 10.0);
    }

    #[test]
    fn optional_mut_skips_when_absent() {
        let mut world = World::new();
        world.spawn((Pos { x: 1.0, y: 0.0 },));

        let mut count = 0;
        for (_, vel) in world.query_mut::<Option<&mut Vel>>() {
            count += 1;
            assert!(vel.is_none());
        }
        assert_eq!(count, 1);
    }

    // -- (&mut A, Option<&mut B>) --

    #[test]
    fn required_mut_plus_optional_mut() {
        let mut world = World::new();
        world.spawn((Pos { x: 1.0, y: 0.0 }, Vel { x: 0.0, y: 0.0 }));
        world.spawn((Pos { x: 2.0, y: 0.0 },)); // no Vel

        for (_, (pos, vel)) in world.query_mut::<(&mut Pos, Option<&mut Vel>)>() {
            pos.x += 100.0;
            if let Some(v) = vel {
                v.x += 50.0;
            }
        }

        let mut results: Vec<_> = world.query::<&Pos>().map(|(_, p)| p.x).collect();
        results.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert_eq!(results, vec![101.0, 102.0]);

        let vel = world.query::<&Vel>().next().unwrap().1;
        assert_eq!(vel.x, 50.0);
    }

    // -- Cross-archetype iteration --

    #[test]
    fn optional_query_spans_multiple_archetypes() {
        let mut world = World::new();
        // Archetype 1: Pos only
        world.spawn((Pos { x: 1.0, y: 0.0 },));
        // Archetype 2: Pos + Vel
        world.spawn((Pos { x: 2.0, y: 0.0 }, Vel { x: 5.0, y: 0.0 }));
        // Archetype 3: Pos + Name
        world.spawn((Pos { x: 3.0, y: 0.0 }, Name("c".into())));

        let mut results: Vec<_> = world
            .query::<(&Pos, Option<&Vel>)>()
            .map(|(_, (pos, vel))| (pos.x, vel.map(|v| v.x)))
            .collect();
        results.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());

        assert_eq!(results.len(), 3);
        assert_eq!(results[0], (1.0, None));
        assert_eq!(results[1], (2.0, Some(5.0)));
        assert_eq!(results[2], (3.0, None));
    }
}
