// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

use crate::archetype::EntityLocation;

/// A lightweight entity identifier with generational indexing.
///
/// The generation field prevents use-after-despawn bugs: if an entity is
/// despawned and its slot reused, the old `Entity` handle will fail
/// `is_alive` checks because the generation won't match.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Entity {
    pub(crate) index: u32,
    pub(crate) generation: u32,
}

impl Entity {
    /// Returns the index portion of this entity ID.
    pub fn index(self) -> u32 {
        self.index
    }

    /// Returns the generation portion of this entity ID.
    pub fn generation(self) -> u32 {
        self.generation
    }
}

// ---------------------------------------------------------------------------
// EntityMeta
// ---------------------------------------------------------------------------

/// Per-slot metadata for an entity: generation and archetype location.
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub(crate) struct EntityMeta {
    /// Current generation for this slot. Incremented on each dealloc.
    pub generation: u32,
    /// Whether this slot is alive.
    pub alive: bool,
    /// Where this entity lives in archetype storage. `None` until placed.
    pub location: Option<EntityLocation>,
}

#[allow(dead_code)]
impl EntityMeta {
    fn new_alive() -> Self {
        Self {
            generation: 0,
            alive: true,
            location: None,
        }
    }
}

// ---------------------------------------------------------------------------
// EntityMetaStore
// ---------------------------------------------------------------------------

/// Manages entity allocation, generation tracking, and archetype location.
///
/// Replaces `EntityAllocator` with added location tracking for archetype
/// storage. Maintains the same public contract for alloc/dealloc/is_alive.
#[allow(dead_code)]
pub(crate) struct EntityMetaStore {
    metas: Vec<EntityMeta>,
    free: Vec<u32>,
}

#[allow(dead_code)]
impl EntityMetaStore {
    pub fn new() -> Self {
        Self {
            metas: Vec::new(),
            free: Vec::new(),
        }
    }

    /// Allocate a new entity, reusing a freed slot if available.
    pub fn alloc(&mut self) -> Entity {
        if let Some(index) = self.free.pop() {
            let meta = &mut self.metas[index as usize];
            meta.alive = true;
            meta.location = None;
            Entity {
                index,
                generation: meta.generation,
            }
        } else {
            let index = self.metas.len() as u32;
            self.metas.push(EntityMeta::new_alive());
            Entity {
                index,
                generation: 0,
            }
        }
    }

    /// Deallocate an entity. Returns `true` if it was alive.
    pub fn dealloc(&mut self, entity: Entity) -> bool {
        let idx = entity.index as usize;
        if idx < self.metas.len() {
            let meta = &mut self.metas[idx];
            if meta.generation == entity.generation && meta.alive {
                meta.alive = false;
                meta.generation += 1;
                meta.location = None;
                self.free.push(entity.index);
                return true;
            }
        }
        false
    }

    /// Check whether an entity handle is still alive.
    pub fn is_alive(&self, entity: Entity) -> bool {
        let idx = entity.index as usize;
        idx < self.metas.len()
            && self.metas[idx].generation == entity.generation
            && self.metas[idx].alive
    }

    /// Returns the total number of allocated slots (including dead).
    pub fn len(&self) -> usize {
        self.metas.len()
    }

    /// Set the archetype location for an entity.
    pub fn set_location(&mut self, entity: Entity, location: EntityLocation) {
        let idx = entity.index as usize;
        debug_assert!(self.is_alive(entity), "set_location on dead entity");
        self.metas[idx].location = Some(location);
    }

    /// Get the archetype location for an entity.
    pub fn get_location(&self, entity: Entity) -> Option<EntityLocation> {
        let idx = entity.index as usize;
        if self.is_alive(entity) {
            self.metas[idx].location
        } else {
            None
        }
    }

    /// Returns an `Entity` handle for a given index, if it's alive.
    pub fn entity_at(&self, index: u32) -> Option<Entity> {
        let idx = index as usize;
        if idx < self.metas.len() && self.metas[idx].alive {
            Some(Entity {
                index,
                generation: self.metas[idx].generation,
            })
        } else {
            None
        }
    }

    /// Returns an iterator over all alive entity handles.
    pub fn alive_entities(&self) -> impl Iterator<Item = Entity> + '_ {
        self.metas
            .iter()
            .enumerate()
            .filter(|(_, meta)| meta.alive)
            .map(|(i, meta)| Entity {
                index: i as u32,
                generation: meta.generation,
            })
    }
}

// ---------------------------------------------------------------------------
// Legacy EntityAllocator (kept until World migration)
// ---------------------------------------------------------------------------

#[allow(dead_code)]
/// Allocates and recycles entity IDs with generational indices.
pub(crate) struct EntityAllocator {
    /// Parallel arrays: `generations[i]` is the current generation for slot `i`.
    generations: Vec<u32>,
    /// Free list of recycled slot indices.
    free: Vec<u32>,
    /// Tracks which slots are alive for iteration.
    alive: Vec<bool>,
}

#[allow(dead_code)]
impl EntityAllocator {
    pub fn new() -> Self {
        Self {
            generations: Vec::new(),
            free: Vec::new(),
            alive: Vec::new(),
        }
    }

    /// Allocate a new entity, reusing a freed slot if available.
    pub fn alloc(&mut self) -> Entity {
        if let Some(index) = self.free.pop() {
            self.alive[index as usize] = true;
            Entity {
                index,
                generation: self.generations[index as usize],
            }
        } else {
            let index = self.generations.len() as u32;
            self.generations.push(0);
            self.alive.push(true);
            Entity {
                index,
                generation: 0,
            }
        }
    }

    /// Deallocate an entity. Returns `true` if it was alive.
    pub fn dealloc(&mut self, entity: Entity) -> bool {
        let idx = entity.index as usize;
        if idx < self.generations.len()
            && self.generations[idx] == entity.generation
            && self.alive[idx]
        {
            self.alive[idx] = false;
            self.generations[idx] += 1;
            self.free.push(entity.index);
            true
        } else {
            false
        }
    }

    /// Check whether an entity handle is still alive.
    pub fn is_alive(&self, entity: Entity) -> bool {
        let idx = entity.index as usize;
        idx < self.generations.len()
            && self.generations[idx] == entity.generation
            && self.alive[idx]
    }

    /// Returns the total number of allocated slots (including dead).
    pub fn len(&self) -> usize {
        self.generations.len()
    }

    /// Returns an `Entity` handle for a given index, if it's alive.
    pub fn entity_at(&self, index: u32) -> Option<Entity> {
        let idx = index as usize;
        if idx < self.generations.len() && self.alive[idx] {
            Some(Entity {
                index,
                generation: self.generations[idx],
            })
        } else {
            None
        }
    }

    /// Returns an iterator over all alive entity handles.
    pub fn alive_entities(&self) -> impl Iterator<Item = Entity> + '_ {
        self.alive
            .iter()
            .enumerate()
            .filter(|(_, alive)| **alive)
            .map(|(i, _)| Entity {
                index: i as u32,
                generation: self.generations[i],
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archetype::{ArchetypeId, EntityLocation};

    // ---- EntityAllocator (legacy, ported) ---------------------------------

    #[test]
    fn alloc_returns_sequential_indices() {
        let mut alloc = EntityAllocator::new();
        let e0 = alloc.alloc();
        let e1 = alloc.alloc();
        assert_eq!(e0.index, 0);
        assert_eq!(e1.index, 1);
        assert_eq!(e0.generation, 0);
        assert_eq!(e1.generation, 0);
    }

    #[test]
    fn dealloc_and_reuse_bumps_generation() {
        let mut alloc = EntityAllocator::new();
        let e0 = alloc.alloc();
        assert!(alloc.dealloc(e0));

        let e0_reused = alloc.alloc();
        assert_eq!(e0_reused.index, 0);
        assert_eq!(e0_reused.generation, 1);
    }

    #[test]
    fn is_alive_returns_false_after_dealloc() {
        let mut alloc = EntityAllocator::new();
        let e = alloc.alloc();
        assert!(alloc.is_alive(e));
        alloc.dealloc(e);
        assert!(!alloc.is_alive(e));
    }

    #[test]
    fn stale_handle_is_not_alive() {
        let mut alloc = EntityAllocator::new();
        let old = alloc.alloc();
        alloc.dealloc(old);
        let _new = alloc.alloc(); // reuses slot 0 with gen 1
        assert!(!alloc.is_alive(old)); // old handle (gen 0) is stale
    }

    #[test]
    fn double_dealloc_returns_false() {
        let mut alloc = EntityAllocator::new();
        let e = alloc.alloc();
        assert!(alloc.dealloc(e));
        assert!(!alloc.dealloc(e));
    }

    #[test]
    fn alive_entities_iterates_only_living() {
        let mut alloc = EntityAllocator::new();
        let _e0 = alloc.alloc();
        let e1 = alloc.alloc();
        let _e2 = alloc.alloc();
        alloc.dealloc(e1);

        let alive: Vec<_> = alloc.alive_entities().collect();
        assert_eq!(alive.len(), 2);
        assert_eq!(alive[0].index, 0);
        assert_eq!(alive[1].index, 2);
    }

    // ---- EntityMetaStore --------------------------------------------------

    #[test]
    fn meta_store_alloc_returns_sequential_indices() {
        let mut store = EntityMetaStore::new();
        let e0 = store.alloc();
        let e1 = store.alloc();
        assert_eq!(e0.index, 0);
        assert_eq!(e1.index, 1);
        assert_eq!(e0.generation, 0);
        assert_eq!(e1.generation, 0);
    }

    #[test]
    fn meta_store_dealloc_and_reuse_bumps_generation() {
        let mut store = EntityMetaStore::new();
        let e0 = store.alloc();
        assert!(store.dealloc(e0));

        let e0_reused = store.alloc();
        assert_eq!(e0_reused.index, 0);
        assert_eq!(e0_reused.generation, 1);
    }

    #[test]
    fn meta_store_is_alive_returns_false_after_dealloc() {
        let mut store = EntityMetaStore::new();
        let e = store.alloc();
        assert!(store.is_alive(e));
        store.dealloc(e);
        assert!(!store.is_alive(e));
    }

    #[test]
    fn meta_store_stale_handle_is_not_alive() {
        let mut store = EntityMetaStore::new();
        let old = store.alloc();
        store.dealloc(old);
        let _new = store.alloc();
        assert!(!store.is_alive(old));
    }

    #[test]
    fn meta_store_double_dealloc_returns_false() {
        let mut store = EntityMetaStore::new();
        let e = store.alloc();
        assert!(store.dealloc(e));
        assert!(!store.dealloc(e));
    }

    #[test]
    fn meta_store_alive_entities_iterates_only_living() {
        let mut store = EntityMetaStore::new();
        let _e0 = store.alloc();
        let e1 = store.alloc();
        let _e2 = store.alloc();
        store.dealloc(e1);

        let alive: Vec<_> = store.alive_entities().collect();
        assert_eq!(alive.len(), 2);
        assert_eq!(alive[0].index, 0);
        assert_eq!(alive[1].index, 2);
    }

    #[test]
    fn meta_store_location_tracking() {
        let mut store = EntityMetaStore::new();
        let e = store.alloc();

        // No location initially.
        assert_eq!(store.get_location(e), None);

        let loc = EntityLocation {
            archetype_id: ArchetypeId(0),
            row: 3,
        };
        store.set_location(e, loc);
        assert_eq!(store.get_location(e), Some(loc));

        // Dealloc clears location.
        store.dealloc(e);
        assert_eq!(store.get_location(e), None);
    }

    #[test]
    fn meta_store_realloc_clears_location() {
        let mut store = EntityMetaStore::new();
        let e = store.alloc();
        store.set_location(
            e,
            EntityLocation {
                archetype_id: ArchetypeId(5),
                row: 2,
            },
        );
        store.dealloc(e);

        let e2 = store.alloc(); // reuses slot 0
        assert_eq!(e2.index, 0);
        assert_eq!(store.get_location(e2), None); // location cleared
    }
}
