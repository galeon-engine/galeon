// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

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
}
