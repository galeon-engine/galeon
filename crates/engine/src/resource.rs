// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

use std::any::{Any, TypeId, type_name};
use std::collections::HashMap;

#[allow(dead_code)]
/// Typed singleton storage for world-global data (e.g., delta time, tick count).
pub(crate) struct Resources {
    map: HashMap<TypeId, Box<dyn Any>>,
}

#[allow(dead_code)]
impl Resources {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    /// Insert a resource, replacing any previous value of the same type.
    pub fn insert<T: 'static>(&mut self, value: T) {
        self.map.insert(TypeId::of::<T>(), Box::new(value));
    }

    /// Get a reference to a resource. Panics if not present.
    pub fn get<T: 'static>(&self) -> &T {
        self.map
            .get(&TypeId::of::<T>())
            .unwrap_or_else(|| panic!("resource not found: {}", type_name::<T>()))
            .downcast_ref::<T>()
            .unwrap()
    }

    /// Get a mutable reference to a resource. Panics if not present.
    pub fn get_mut<T: 'static>(&mut self) -> &mut T {
        self.map
            .get_mut(&TypeId::of::<T>())
            .unwrap_or_else(|| panic!("resource not found: {}", type_name::<T>()))
            .downcast_mut::<T>()
            .unwrap()
    }

    /// Try to get a reference to a resource. Returns `None` if not present.
    pub fn try_get<T: 'static>(&self) -> Option<&T> {
        self.map
            .get(&TypeId::of::<T>())
            .and_then(|v| v.downcast_ref::<T>())
    }

    /// Remove and return a resource. Panics if not present.
    pub fn take<T: 'static>(&mut self) -> T {
        *self
            .map
            .remove(&TypeId::of::<T>())
            .unwrap_or_else(|| panic!("resource not found: {}", type_name::<T>()))
            .downcast::<T>()
            .unwrap()
    }

    /// Returns `true` if the resource exists.
    pub fn contains<T: 'static>(&self) -> bool {
        self.map.contains_key(&TypeId::of::<T>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_get() {
        let mut res = Resources::new();
        res.insert(42_i32);
        assert_eq!(*res.get::<i32>(), 42);
    }

    #[test]
    fn get_mut() {
        let mut res = Resources::new();
        res.insert(10_u32);
        *res.get_mut::<u32>() = 20;
        assert_eq!(*res.get::<u32>(), 20);
    }

    #[test]
    fn overwrite() {
        let mut res = Resources::new();
        res.insert(1_i32);
        res.insert(2_i32);
        assert_eq!(*res.get::<i32>(), 2);
    }

    #[test]
    fn try_get_missing_returns_none() {
        let res = Resources::new();
        assert!(res.try_get::<f64>().is_none());
    }

    #[test]
    #[should_panic(expected = "resource not found")]
    fn get_missing_panics() {
        let res = Resources::new();
        let _: &f64 = res.get::<f64>();
    }

    #[test]
    fn contains() {
        let mut res = Resources::new();
        assert!(!res.contains::<i32>());
        res.insert(1_i32);
        assert!(res.contains::<i32>());
    }
}
