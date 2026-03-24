// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

use std::any::{Any, TypeId};
use std::collections::HashMap;

use crate::entity::Entity;

// ---------------------------------------------------------------------------
// ArchetypeId
// ---------------------------------------------------------------------------

/// Index into the archetype store.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ArchetypeId(pub(crate) u32);

impl ArchetypeId {
    /// Returns the raw index.
    pub fn index(self) -> u32 {
        self.0
    }
}

// ---------------------------------------------------------------------------
// EntityLocation
// ---------------------------------------------------------------------------

/// Where an entity lives inside archetype storage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EntityLocation {
    pub archetype_id: ArchetypeId,
    pub row: u32,
}

// ---------------------------------------------------------------------------
// ArchetypeLayout
// ---------------------------------------------------------------------------

/// Sorted set of `TypeId`s identifying which components an archetype holds.
///
/// Two layouts with the same type set (regardless of input order) are equal
/// and hash the same.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ArchetypeLayout {
    /// Always kept sorted and deduplicated.
    type_ids: Vec<TypeId>,
}

impl ArchetypeLayout {
    /// Create a layout from an unsorted, possibly-duplicate slice of type IDs.
    pub fn from_type_ids(ids: &[TypeId]) -> Self {
        let mut sorted = ids.to_vec();
        sorted.sort();
        sorted.dedup();
        Self { type_ids: sorted }
    }

    /// Create an empty layout (the "void" archetype for entities with no components).
    pub fn empty() -> Self {
        Self {
            type_ids: Vec::new(),
        }
    }

    /// Whether this layout contains the given type.
    pub fn contains(&self, id: TypeId) -> bool {
        self.type_ids.binary_search(&id).is_ok()
    }

    /// Return a new layout with `id` added (no-op if already present).
    pub fn with_added(&self, id: TypeId) -> Self {
        if self.contains(id) {
            return self.clone();
        }
        let mut ids = self.type_ids.clone();
        let pos = ids.binary_search(&id).unwrap_err();
        ids.insert(pos, id);
        Self { type_ids: ids }
    }

    /// Return a new layout with `id` removed (no-op if absent).
    pub fn with_removed(&self, id: TypeId) -> Self {
        if let Ok(pos) = self.type_ids.binary_search(&id) {
            let mut ids = self.type_ids.clone();
            ids.remove(pos);
            Self { type_ids: ids }
        } else {
            self.clone()
        }
    }

    /// The number of component types in this layout.
    pub fn len(&self) -> usize {
        self.type_ids.len()
    }

    /// Whether this layout has zero component types.
    pub fn is_empty(&self) -> bool {
        self.type_ids.is_empty()
    }

    /// Iterate the type IDs in sorted order.
    pub fn iter(&self) -> impl Iterator<Item = TypeId> + '_ {
        self.type_ids.iter().copied()
    }
}

// ---------------------------------------------------------------------------
// AnyColumn trait + Column<T>
// ---------------------------------------------------------------------------

/// Type-erased column operations. Each archetype column implements this.
#[allow(clippy::len_without_is_empty)]
pub trait AnyColumn: Any + Send + Sync {
    /// Swap-remove a row. The data is dropped.
    fn swap_remove_and_drop(&mut self, row: usize);

    /// Move the data at `row` in this column into `dst` (which must be the
    /// same concrete `Column<T>`). The row is swap-removed from `self`.
    fn move_to(&mut self, row: usize, dst: &mut dyn AnyColumn);

    /// Number of rows.
    fn len(&self) -> usize;

    /// Create an empty column of the same concrete type.
    fn new_empty(&self) -> Box<dyn AnyColumn>;

    // Down-casting helpers.
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

/// Typed dense column for a single component type within one archetype.
pub struct Column<T> {
    data: Vec<T>,
}

impl<T: Send + Sync + 'static> Default for Column<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Send + Sync + 'static> Column<T> {
    /// Create an empty column.
    pub fn new() -> Self {
        Self { data: Vec::new() }
    }

    /// Append a value at the end.
    pub fn push(&mut self, value: T) {
        self.data.push(value);
    }

    /// Get an immutable reference by row index.
    pub fn get(&self, row: usize) -> Option<&T> {
        self.data.get(row)
    }

    /// Get a mutable reference by row index.
    pub fn get_mut(&mut self, row: usize) -> Option<&mut T> {
        self.data.get_mut(row)
    }

    /// Swap-remove and return the value at `row`.
    pub fn swap_remove(&mut self, row: usize) -> T {
        self.data.swap_remove(row)
    }

    /// Number of rows.
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Whether the column is empty.
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

impl<T: Send + Sync + 'static> AnyColumn for Column<T> {
    fn swap_remove_and_drop(&mut self, row: usize) {
        self.data.swap_remove(row);
    }

    fn move_to(&mut self, row: usize, dst: &mut dyn AnyColumn) {
        let value = self.data.swap_remove(row);
        let dst_typed = dst
            .as_any_mut()
            .downcast_mut::<Column<T>>()
            .expect("move_to: column type mismatch");
        dst_typed.data.push(value);
    }

    fn len(&self) -> usize {
        self.data.len()
    }

    fn new_empty(&self) -> Box<dyn AnyColumn> {
        Box::new(Column::<T>::new())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// ---------------------------------------------------------------------------
// ArchetypeEdge
// ---------------------------------------------------------------------------

/// Cached archetype transition when a component is added or removed.
#[derive(Clone, Debug, Default)]
pub struct ArchetypeEdge {
    /// Archetype reached by adding a component of this type.
    pub add: Option<ArchetypeId>,
    /// Archetype reached by removing a component of this type.
    pub remove: Option<ArchetypeId>,
}

// ---------------------------------------------------------------------------
// Archetype
// ---------------------------------------------------------------------------

/// A group of entities that share the same set of component types.
///
/// Columns are independently borrowable — no double-borrow of a `HashMap`
/// needed for multi-component queries.
#[allow(dead_code)]
pub struct Archetype {
    id: ArchetypeId,
    layout: ArchetypeLayout,
    /// Entity handles stored in insertion order; indices are row numbers.
    entities: Vec<Entity>,
    /// One dense column per component type in the layout.
    columns: HashMap<TypeId, Box<dyn AnyColumn>>,
    /// Lazy edge cache for archetype transitions.
    edges: HashMap<TypeId, ArchetypeEdge>,
}

#[allow(dead_code)]
impl Archetype {
    /// Create a new empty archetype. Each type in `layout` gets an empty column
    /// created by `column_factories` — one factory per `TypeId`.
    ///
    /// `column_factories` maps each `TypeId` to a function that produces an
    /// empty `Box<dyn AnyColumn>` of the right concrete type. The caller
    /// must provide a factory for every type in the layout.
    pub(crate) fn new(
        id: ArchetypeId,
        layout: ArchetypeLayout,
        column_factories: &HashMap<TypeId, Box<dyn AnyColumn>>,
    ) -> Self {
        let mut columns = HashMap::new();
        for tid in layout.iter() {
            let template = column_factories
                .get(&tid)
                .unwrap_or_else(|| panic!("no column factory for {:?}", tid));
            columns.insert(tid, template.new_empty());
        }
        Self {
            id,
            layout,
            entities: Vec::new(),
            columns,
            edges: HashMap::new(),
        }
    }

    /// The archetype's ID.
    pub fn id(&self) -> ArchetypeId {
        self.id
    }

    /// The archetype's layout.
    pub fn layout(&self) -> &ArchetypeLayout {
        &self.layout
    }

    /// Number of entities in this archetype.
    pub fn len(&self) -> usize {
        self.entities.len()
    }

    /// Whether this archetype has no entities.
    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    /// The entity stored at `row`.
    pub fn entity_at(&self, row: usize) -> Entity {
        self.entities[row]
    }

    /// Slice of all entities in this archetype.
    pub fn entities(&self) -> &[Entity] {
        &self.entities
    }

    /// Get a typed column reference. Returns `None` if the type isn't in this
    /// archetype's layout.
    pub fn column<T: Send + Sync + 'static>(&self) -> Option<&Column<T>> {
        let col = self.columns.get(&TypeId::of::<T>())?;
        col.as_any().downcast_ref::<Column<T>>()
    }

    /// Get a typed column mutable reference.
    pub fn column_mut<T: Send + Sync + 'static>(&mut self) -> Option<&mut Column<T>> {
        let col = self.columns.get_mut(&TypeId::of::<T>())?;
        col.as_any_mut().downcast_mut::<Column<T>>()
    }

    /// Get a type-erased column reference.
    pub fn column_raw(&self, type_id: TypeId) -> Option<&dyn AnyColumn> {
        self.columns.get(&type_id).map(|c| &**c)
    }

    /// Get a type-erased column mutable reference.
    pub fn column_raw_mut(&mut self, type_id: TypeId) -> Option<&mut dyn AnyColumn> {
        self.columns.get_mut(&type_id).map(|c| &mut **c)
    }

    /// Append an entity. The caller must push one value into every column
    /// *before* calling this (or use the higher-level `ArchetypeStore` API).
    ///
    /// Returns the row index where the entity was placed.
    pub(crate) fn push_entity(&mut self, entity: Entity) -> u32 {
        let row = self.entities.len() as u32;
        self.entities.push(entity);
        row
    }

    /// Swap-remove the entity at `row`, dropping all its component data.
    /// Returns the entity that was removed *and* the entity that was moved
    /// into `row` (if any — `None` when the removed entity was the last).
    pub(crate) fn swap_remove_entity(&mut self, row: usize) -> (Entity, Option<Entity>) {
        let removed = self.entities.swap_remove(row);
        let moved = if row < self.entities.len() {
            Some(self.entities[row])
        } else {
            None
        };
        for col in self.columns.values_mut() {
            col.swap_remove_and_drop(row);
        }
        (removed, moved)
    }

    /// Get the edge cache entry for a component type.
    pub fn edge(&self, type_id: TypeId) -> Option<&ArchetypeEdge> {
        self.edges.get(&type_id)
    }

    /// Insert or update an edge cache entry.
    pub(crate) fn set_edge(&mut self, type_id: TypeId, edge: ArchetypeEdge) {
        self.edges.insert(type_id, edge);
    }
}

// ---------------------------------------------------------------------------
// ArchetypeStore
// ---------------------------------------------------------------------------

/// Registry of all archetypes, indexed by layout.
pub struct ArchetypeStore {
    archetypes: Vec<Archetype>,
    index: HashMap<ArchetypeLayout, ArchetypeId>,
    /// Column factories: for each TypeId that has ever been seen, a prototype
    /// `Box<dyn AnyColumn>` that can produce empty columns via `new_empty()`.
    column_factories: HashMap<TypeId, Box<dyn AnyColumn>>,
}

impl ArchetypeStore {
    /// Create an empty store.
    pub fn new() -> Self {
        Self {
            archetypes: Vec::new(),
            index: HashMap::new(),
            column_factories: HashMap::new(),
        }
    }

    /// Register a column factory for a component type. Must be called before
    /// `get_or_create` is called with a layout containing that type.
    pub fn register_column<T: Send + Sync + 'static>(&mut self) {
        self.column_factories
            .entry(TypeId::of::<T>())
            .or_insert_with(|| Box::new(Column::<T>::new()));
    }

    /// Look up or create the archetype for `layout`.
    pub fn get_or_create(&mut self, layout: ArchetypeLayout) -> ArchetypeId {
        if let Some(&id) = self.index.get(&layout) {
            return id;
        }
        let id = ArchetypeId(self.archetypes.len() as u32);
        let archetype = Archetype::new(id, layout.clone(), &self.column_factories);
        self.archetypes.push(archetype);
        self.index.insert(layout, id);
        id
    }

    /// Get an archetype by ID.
    pub fn get(&self, id: ArchetypeId) -> &Archetype {
        &self.archetypes[id.0 as usize]
    }

    /// Get a mutable archetype by ID.
    pub fn get_mut(&mut self, id: ArchetypeId) -> &mut Archetype {
        &mut self.archetypes[id.0 as usize]
    }

    /// Number of archetypes.
    pub fn len(&self) -> usize {
        self.archetypes.len()
    }

    /// Whether the store has no archetypes.
    pub fn is_empty(&self) -> bool {
        self.archetypes.is_empty()
    }

    /// Iterate all archetypes.
    pub fn iter(&self) -> impl Iterator<Item = &Archetype> {
        self.archetypes.iter()
    }
}

impl Default for ArchetypeStore {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ---- ArchetypeLayout --------------------------------------------------

    #[test]
    fn layout_sorts_and_deduplicates() {
        let a = TypeId::of::<u32>();
        let b = TypeId::of::<f64>();
        let l1 = ArchetypeLayout::from_type_ids(&[b, a, b, a]);
        let l2 = ArchetypeLayout::from_type_ids(&[a, b]);
        assert_eq!(l1, l2);
        assert_eq!(l1.len(), 2);
    }

    #[test]
    fn layout_contains() {
        let layout = ArchetypeLayout::from_type_ids(&[TypeId::of::<u32>(), TypeId::of::<f64>()]);
        assert!(layout.contains(TypeId::of::<u32>()));
        assert!(layout.contains(TypeId::of::<f64>()));
        assert!(!layout.contains(TypeId::of::<bool>()));
    }

    #[test]
    fn layout_with_added_and_removed() {
        let base = ArchetypeLayout::from_type_ids(&[TypeId::of::<u32>()]);
        let added = base.with_added(TypeId::of::<f64>());
        assert_eq!(added.len(), 2);
        assert!(added.contains(TypeId::of::<f64>()));

        // Adding an already-present type is a no-op.
        let same = added.with_added(TypeId::of::<u32>());
        assert_eq!(same, added);

        let removed = added.with_removed(TypeId::of::<u32>());
        assert_eq!(removed.len(), 1);
        assert!(!removed.contains(TypeId::of::<u32>()));
        assert!(removed.contains(TypeId::of::<f64>()));

        // Removing an absent type is a no-op.
        let same2 = removed.with_removed(TypeId::of::<bool>());
        assert_eq!(same2, removed);
    }

    #[test]
    fn layout_empty() {
        let e = ArchetypeLayout::empty();
        assert!(e.is_empty());
        assert_eq!(e.len(), 0);
    }

    #[test]
    fn layout_hash_eq_regardless_of_input_order() {
        use std::hash::{DefaultHasher, Hash, Hasher};
        let a = TypeId::of::<u32>();
        let b = TypeId::of::<f64>();
        let c = TypeId::of::<bool>();

        let l1 = ArchetypeLayout::from_type_ids(&[c, a, b]);
        let l2 = ArchetypeLayout::from_type_ids(&[b, c, a]);

        assert_eq!(l1, l2);

        let hash = |l: &ArchetypeLayout| {
            let mut h = DefaultHasher::new();
            l.hash(&mut h);
            h.finish()
        };
        assert_eq!(hash(&l1), hash(&l2));
    }

    // ---- Column<T> --------------------------------------------------------

    #[test]
    fn column_push_get() {
        let mut col = Column::<u32>::new();
        col.push(10);
        col.push(20);
        col.push(30);
        assert_eq!(col.len(), 3);
        assert_eq!(*col.get(0).unwrap(), 10);
        assert_eq!(*col.get(1).unwrap(), 20);
        assert_eq!(*col.get(2).unwrap(), 30);
    }

    #[test]
    fn column_get_mut() {
        let mut col = Column::<u32>::new();
        col.push(5);
        *col.get_mut(0).unwrap() = 99;
        assert_eq!(*col.get(0).unwrap(), 99);
    }

    #[test]
    fn column_swap_remove() {
        let mut col = Column::<&str>::new();
        col.push("a");
        col.push("b");
        col.push("c");
        let removed = col.swap_remove(0);
        assert_eq!(removed, "a");
        assert_eq!(col.len(), 2);
        // "c" moved into row 0
        assert_eq!(*col.get(0).unwrap(), "c");
        assert_eq!(*col.get(1).unwrap(), "b");
    }

    #[test]
    fn column_move_to() {
        let mut src = Column::<u32>::new();
        src.push(10);
        src.push(20);
        src.push(30);

        let mut dst = Column::<u32>::new();
        src.move_to(1, &mut dst); // moves 20, swap-removes from src (30 fills row 1)

        assert_eq!(src.len(), 2);
        assert_eq!(dst.len(), 1);
        assert_eq!(*dst.get(0).unwrap(), 20);
        assert_eq!(*src.get(0).unwrap(), 10);
        assert_eq!(*src.get(1).unwrap(), 30);
    }

    #[test]
    fn column_new_empty_produces_correct_type() {
        let col = Column::<f64>::new();
        let any_col: &dyn AnyColumn = &col;
        let empty = any_col.new_empty();
        assert_eq!(empty.len(), 0);
        // Downcast succeeds to the same type.
        assert!(empty.as_any().downcast_ref::<Column<f64>>().is_some());
    }

    // ---- Archetype + ArchetypeStore ---------------------------------------

    #[test]
    fn archetype_store_get_or_create_idempotent() {
        let mut store = ArchetypeStore::new();
        store.register_column::<u32>();
        store.register_column::<f64>();

        let layout = ArchetypeLayout::from_type_ids(&[TypeId::of::<u32>(), TypeId::of::<f64>()]);
        let id1 = store.get_or_create(layout.clone());
        let id2 = store.get_or_create(layout);
        assert_eq!(id1, id2);
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn archetype_push_and_remove_entity() {
        let mut store = ArchetypeStore::new();
        store.register_column::<u32>();
        store.register_column::<String>();

        let layout = ArchetypeLayout::from_type_ids(&[TypeId::of::<u32>(), TypeId::of::<String>()]);
        let arch_id = store.get_or_create(layout);

        let e0 = Entity {
            index: 0,
            generation: 0,
        };
        let e1 = Entity {
            index: 1,
            generation: 0,
        };
        let e2 = Entity {
            index: 2,
            generation: 0,
        };

        // Push three entities.
        {
            let arch = store.get_mut(arch_id);
            arch.column_mut::<u32>().unwrap().push(10);
            arch.column_mut::<String>()
                .unwrap()
                .push("hello".to_string());
            arch.push_entity(e0);

            arch.column_mut::<u32>().unwrap().push(20);
            arch.column_mut::<String>()
                .unwrap()
                .push("world".to_string());
            arch.push_entity(e1);

            arch.column_mut::<u32>().unwrap().push(30);
            arch.column_mut::<String>().unwrap().push("foo".to_string());
            arch.push_entity(e2);

            assert_eq!(arch.len(), 3);
        }

        // Remove entity at row 0 (e0). e2 (last) swaps into row 0.
        {
            let arch = store.get_mut(arch_id);
            let (removed, moved) = arch.swap_remove_entity(0);
            assert_eq!(removed, e0);
            assert_eq!(moved, Some(e2)); // e2 moved into row 0

            assert_eq!(arch.len(), 2);
            assert_eq!(arch.entity_at(0), e2);
            assert_eq!(arch.entity_at(1), e1);

            // Column data consistent after swap-remove.
            assert_eq!(*arch.column::<u32>().unwrap().get(0).unwrap(), 30);
            assert_eq!(*arch.column::<u32>().unwrap().get(1).unwrap(), 20);
        }
    }

    #[test]
    fn archetype_remove_last_entity_returns_no_moved() {
        let mut store = ArchetypeStore::new();
        store.register_column::<u32>();

        let layout = ArchetypeLayout::from_type_ids(&[TypeId::of::<u32>()]);
        let arch_id = store.get_or_create(layout);

        let e0 = Entity {
            index: 0,
            generation: 0,
        };

        let arch = store.get_mut(arch_id);
        arch.column_mut::<u32>().unwrap().push(42);
        arch.push_entity(e0);

        let (removed, moved) = arch.swap_remove_entity(0);
        assert_eq!(removed, e0);
        assert!(moved.is_none());
        assert!(arch.is_empty());
    }

    #[test]
    fn archetype_column_len_matches_entity_count() {
        let mut store = ArchetypeStore::new();
        store.register_column::<u32>();
        store.register_column::<bool>();

        let layout = ArchetypeLayout::from_type_ids(&[TypeId::of::<u32>(), TypeId::of::<bool>()]);
        let arch_id = store.get_or_create(layout);

        let arch = store.get_mut(arch_id);
        for i in 0..5 {
            arch.column_mut::<u32>().unwrap().push(i);
            arch.column_mut::<bool>().unwrap().push(i % 2 == 0);
            arch.push_entity(Entity {
                index: i,
                generation: 0,
            });
        }

        assert_eq!(arch.len(), 5);
        assert_eq!(arch.column::<u32>().unwrap().len(), 5);
        assert_eq!(arch.column::<bool>().unwrap().len(), 5);
    }

    #[test]
    fn archetype_store_multiple_layouts() {
        let mut store = ArchetypeStore::new();
        store.register_column::<u32>();
        store.register_column::<f64>();
        store.register_column::<bool>();

        let l1 = ArchetypeLayout::from_type_ids(&[TypeId::of::<u32>()]);
        let l2 = ArchetypeLayout::from_type_ids(&[TypeId::of::<u32>(), TypeId::of::<f64>()]);
        let l3 = ArchetypeLayout::from_type_ids(&[
            TypeId::of::<u32>(),
            TypeId::of::<f64>(),
            TypeId::of::<bool>(),
        ]);

        let id1 = store.get_or_create(l1);
        let id2 = store.get_or_create(l2);
        let id3 = store.get_or_create(l3);

        assert_ne!(id1, id2);
        assert_ne!(id2, id3);
        assert_eq!(store.len(), 3);
    }

    // ---- Edge cache -------------------------------------------------------

    #[test]
    fn edge_cache_starts_empty() {
        let mut store = ArchetypeStore::new();
        store.register_column::<u32>();

        let layout = ArchetypeLayout::from_type_ids(&[TypeId::of::<u32>()]);
        let arch_id = store.get_or_create(layout);
        let arch = store.get(arch_id);

        assert!(arch.edge(TypeId::of::<f64>()).is_none());
    }

    #[test]
    fn edge_cache_set_and_get() {
        let mut store = ArchetypeStore::new();
        store.register_column::<u32>();
        store.register_column::<f64>();

        let l1 = ArchetypeLayout::from_type_ids(&[TypeId::of::<u32>()]);
        let l2 = ArchetypeLayout::from_type_ids(&[TypeId::of::<u32>(), TypeId::of::<f64>()]);
        let id1 = store.get_or_create(l1);
        let id2 = store.get_or_create(l2);

        // Simulate: adding f64 to archetype 1 → archetype 2.
        store.get_mut(id1).set_edge(
            TypeId::of::<f64>(),
            ArchetypeEdge {
                add: Some(id2),
                remove: None,
            },
        );

        let edge = store.get(id1).edge(TypeId::of::<f64>()).unwrap();
        assert_eq!(edge.add, Some(id2));
        assert_eq!(edge.remove, None);
    }
}
