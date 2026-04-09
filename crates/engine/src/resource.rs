// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

use std::any::{Any, TypeId, type_name};
use std::cell::UnsafeCell;
use std::collections::HashMap;

#[allow(dead_code)]
/// Typed singleton storage for world-global data (e.g., delta time, tick count).
///
/// Values are wrapped in `UnsafeCell` to support interior mutability without
/// creating overlapping `&Resources` / `&mut Resources`. This eliminates the
/// Stacked Borrows aliasing UB when `UnsafeWorldCell` accesses different
/// resources concurrently (e.g., `Res<A>` + `ResMut<B>`).
pub(crate) struct Resources {
    map: HashMap<TypeId, UnsafeCell<Box<dyn Any + Send>>>,
}

#[allow(dead_code)]
impl Resources {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    /// Insert a resource, replacing any previous value of the same type.
    pub fn insert<T: Send + 'static>(&mut self, value: T) {
        self.map
            .insert(TypeId::of::<T>(), UnsafeCell::new(Box::new(value)));
    }

    /// Get a reference to a resource. Panics if not present.
    pub fn get<T: Send + 'static>(&self) -> &T {
        let cell = self
            .map
            .get(&TypeId::of::<T>())
            .unwrap_or_else(|| panic!("resource not found: {}", type_name::<T>()));
        // SAFETY: We hold `&self`, so no `&mut self` exists — no other code
        // can create a mutable reference through UnsafeCell concurrently.
        let boxed: &Box<dyn Any + Send> = unsafe { &*cell.get() };
        boxed.downcast_ref::<T>().unwrap()
    }

    /// Get a mutable reference to a resource. Panics if not present.
    pub fn get_mut<T: Send + 'static>(&mut self) -> &mut T {
        let cell = self
            .map
            .get(&TypeId::of::<T>())
            .unwrap_or_else(|| panic!("resource not found: {}", type_name::<T>()));
        // SAFETY: We hold `&mut self`, so exclusive access is guaranteed.
        let boxed: &mut Box<dyn Any + Send> = unsafe { &mut *cell.get() };
        boxed.downcast_mut::<T>().unwrap()
    }

    /// Get a reference to a resource using only `&self`.
    ///
    /// This is the `UnsafeWorldCell` path: both `get_unchecked` and
    /// `get_mut_unchecked` take `&self`, so the caller only needs a single
    /// `&Resources` — eliminating `&Resources` / `&mut Resources` overlap.
    ///
    /// # Safety
    ///
    /// - The resource of type `T` must exist.
    /// - No mutable reference to the same resource may exist concurrently.
    pub(crate) unsafe fn get_unchecked<T: Send + 'static>(&self) -> &T {
        let cell = self
            .map
            .get(&TypeId::of::<T>())
            .unwrap_or_else(|| panic!("resource not found: {}", type_name::<T>()));
        // SAFETY: Caller guarantees no mutable reference to this resource.
        unsafe { &*cell.get() }.downcast_ref::<T>().unwrap()
    }

    /// Get a mutable reference to a resource using only `&self`.
    ///
    /// Interior mutability via `UnsafeCell` — the caller only needs `&self`,
    /// so no `&mut Resources` is created. Combined with `get_unchecked`,
    /// this allows `Res<A>` + `ResMut<B>` without overlapping container refs.
    ///
    /// # Safety
    ///
    /// - The resource of type `T` must exist.
    /// - No other reference (shared or mutable) to the same resource may
    ///   exist concurrently.
    #[allow(clippy::mut_from_ref)] // Intentional: UnsafeCell interior mutability
    pub(crate) unsafe fn get_mut_unchecked<T: Send + 'static>(&self) -> &mut T {
        let cell = self
            .map
            .get(&TypeId::of::<T>())
            .unwrap_or_else(|| panic!("resource not found: {}", type_name::<T>()));
        // SAFETY: Caller guarantees exclusive access to this resource.
        // UnsafeCell allows mutation through &self.
        unsafe { &mut *cell.get() }.downcast_mut::<T>().unwrap()
    }

    /// Try to get a reference to a resource. Returns `None` if not present.
    pub fn try_get<T: Send + 'static>(&self) -> Option<&T> {
        let cell = self.map.get(&TypeId::of::<T>())?;
        // SAFETY: We hold `&self`, so no mutable access exists.
        let boxed: &Box<dyn Any + Send> = unsafe { &*cell.get() };
        boxed.downcast_ref::<T>()
    }

    /// Remove and return a resource. Panics if not present.
    pub fn take<T: Send + 'static>(&mut self) -> T {
        *self
            .map
            .remove(&TypeId::of::<T>())
            .unwrap_or_else(|| panic!("resource not found: {}", type_name::<T>()))
            .into_inner()
            .downcast::<T>()
            .unwrap()
    }

    /// Try to remove and return a resource. Returns `None` if not present.
    pub fn try_take<T: Send + 'static>(&mut self) -> Option<T> {
        self.map
            .remove(&TypeId::of::<T>())
            .and_then(|cell| cell.into_inner().downcast::<T>().ok())
            .map(|b| *b)
    }

    /// Returns `true` if the resource exists.
    pub fn contains<T: Send + 'static>(&self) -> bool {
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

    #[test]
    fn get_unchecked_reads_resource() {
        let mut res = Resources::new();
        res.insert(42_i32);
        unsafe {
            assert_eq!(*res.get_unchecked::<i32>(), 42);
        }
    }

    #[test]
    fn get_mut_unchecked_mutates_resource() {
        let mut res = Resources::new();
        res.insert(10_u32);
        unsafe {
            *res.get_mut_unchecked::<u32>() = 20;
        }
        assert_eq!(*res.get::<u32>(), 20);
    }
}
