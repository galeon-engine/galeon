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

    fn init_state<'w>(archetype: &'w mut Archetype) -> Option<Self::State<'w>>;

    fn len(state: &Self::State<'_>) -> usize;

    fn entity(state: &Self::State<'_>, row: usize) -> Entity;

    /// # Safety
    ///
    /// Callers must only request each row at most once per live state. Doing
    /// otherwise could create aliased mutable references into the same
    /// archetype column data.
    unsafe fn fetch<'w>(state: &mut Self::State<'w>, row: usize) -> Self::Item<'w>;
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
    current: Option<Q::State<'w>>,
    _filter: PhantomData<F>,
    _marker: PhantomData<&'w mut ArchetypeStore>,
}

impl<'w, Q, F> QueryIterMut<'w, Q, F>
where
    Q: QuerySpecMut,
    F: QueryFilter,
{
    pub(crate) fn new(store: &'w mut ArchetypeStore) -> Self {
        Self {
            archetype_len: store.len(),
            store,
            archetype_index: 0,
            row: 0,
            current: None,
            _filter: PhantomData,
            _marker: PhantomData,
        }
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
                    return Some((entity, item));
                }
                self.current = None;
            }

            if self.archetype_index >= self.archetype_len {
                return None;
            }

            // SAFETY: The iterator owns the only mutable borrow of the store
            // for `'w`. `current` is cleared before borrowing a new archetype,
            // so mutable state from one archetype is never active while
            // another mutable archetype borrow is created.
            let archetype = unsafe { (&mut *self.store).get_by_index_mut(self.archetype_index)? };
            self.archetype_index += 1;

            if !Q::matches(archetype.layout()) || !F::matches(archetype.layout()) {
                continue;
            }

            self.current = Q::init_state(archetype);
            self.row = 0;
        }
    }
}

#[doc(hidden)]
pub struct SingleMutState<'w, T> {
    entities: &'w [Entity],
    data: *mut T,
    len: usize,
    _marker: PhantomData<&'w mut T>,
}

#[doc(hidden)]
pub struct PairMutState<'w, A, B> {
    entities: &'w [Entity],
    col_a: *mut A,
    col_b: *mut B,
    len: usize,
    _marker: PhantomData<&'w mut (A, B)>,
}

#[doc(hidden)]
pub struct TripleMutState<'w, A, B, C> {
    entities: &'w [Entity],
    col_a: *mut A,
    col_b: *mut B,
    col_c: *mut C,
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

impl<T: Component> QuerySpecMut for &mut T {
    type Item<'w> = &'w mut T;

    type State<'w> = SingleMutState<'w, T>;

    fn matches(layout: &ArchetypeLayout) -> bool {
        layout.contains(TypeId::of::<T>())
    }

    fn init_state<'w>(archetype: &'w mut Archetype) -> Option<Self::State<'w>> {
        let (entities, column) = archetype.entities_and_column_mut::<T>()?;
        Some(SingleMutState {
            entities,
            data: column.as_mut_ptr(),
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
}

impl<A: Component, B: Component> QuerySpecMut for (&mut A, &mut B) {
    type Item<'w> = (&'w mut A, &'w mut B);

    type State<'w> = PairMutState<'w, A, B>;

    fn matches(layout: &ArchetypeLayout) -> bool {
        layout.contains(TypeId::of::<A>()) && layout.contains(TypeId::of::<B>())
    }

    fn init_state<'w>(archetype: &'w mut Archetype) -> Option<Self::State<'w>> {
        let (entities, col_a, col_b) = archetype.entities_and_two_columns_mut::<A, B>()?;
        Some(PairMutState {
            entities,
            col_a: col_a.as_mut_ptr(),
            col_b: col_b.as_mut_ptr(),
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
}

impl<A: Component, B: Component, C: Component> QuerySpecMut for (&mut A, &mut B, &mut C) {
    type Item<'w> = (&'w mut A, &'w mut B, &'w mut C);

    type State<'w> = TripleMutState<'w, A, B, C>;

    fn matches(layout: &ArchetypeLayout) -> bool {
        layout.contains(TypeId::of::<A>())
            && layout.contains(TypeId::of::<B>())
            && layout.contains(TypeId::of::<C>())
    }

    fn init_state<'w>(archetype: &'w mut Archetype) -> Option<Self::State<'w>> {
        let (entities, col_a, col_b, col_c) =
            archetype.entities_and_three_columns_mut::<A, B, C>()?;
        Some(TripleMutState {
            entities,
            col_a: col_a.as_mut_ptr(),
            col_b: col_b.as_mut_ptr(),
            col_c: col_c.as_mut_ptr(),
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
}
