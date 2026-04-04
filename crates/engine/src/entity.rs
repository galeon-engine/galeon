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
    /// Reconstruct an `Entity` handle from its raw index and generation.
    ///
    /// This is intended for the WASM bridge where JS passes back an entity ID
    /// that was previously returned by `spawn`. The caller is responsible for
    /// providing a valid (index, generation) pair — passing stale or fabricated
    /// values is safe but will cause `is_alive` / `get` to return `None`.
    pub fn from_raw(index: u32, generation: u32) -> Self {
        Self { index, generation }
    }

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
#[derive(Clone, Debug)]
pub(crate) struct EntityMeta {
    /// Current generation for this slot. Incremented on each dealloc.
    pub generation: u32,
    /// Whether this slot is alive.
    pub alive: bool,
    /// Where this entity lives in archetype storage. `None` until placed.
    pub location: Option<EntityLocation>,
}

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
pub(crate) struct EntityMetaStore {
    metas: Vec<EntityMeta>,
    free: Vec<u32>,
}

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
    #[allow(dead_code)]
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
    #[allow(dead_code)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archetype::{ArchetypeId, EntityLocation};

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
