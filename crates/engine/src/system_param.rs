// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

use std::any::TypeId;

/// Describes what a system parameter accesses — used for conflict detection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Access {
    /// Shared read of a resource.
    ResRead(TypeId),
    /// Exclusive write of a resource.
    ResWrite(TypeId),
    /// Shared read of a component type.
    CompRead(TypeId),
    /// Exclusive write of a component type.
    CompWrite(TypeId),
}

impl Access {
    /// Returns `true` if `self` and `other` cannot safely coexist in the same
    /// parallel system execution.
    ///
    /// Conflict rules:
    /// - `ResRead`  + `ResWrite`  (same TypeId) → conflict
    /// - `ResWrite` + `ResWrite`  (same TypeId) → conflict
    /// - `CompRead` + `CompWrite` (same TypeId) → conflict
    /// - `CompWrite`+ `CompWrite` (same TypeId) → conflict
    /// - Everything else → no conflict (read-read, different TypeIds, cross-namespace)
    pub fn conflicts_with(&self, other: &Access) -> bool {
        match (self, other) {
            (Access::ResRead(a), Access::ResWrite(b))
            | (Access::ResWrite(a), Access::ResRead(b))
            | (Access::ResWrite(a), Access::ResWrite(b)) => a == b,

            (Access::CompRead(a), Access::CompWrite(b))
            | (Access::CompWrite(a), Access::CompRead(b))
            | (Access::CompWrite(a), Access::CompWrite(b)) => a == b,

            _ => false,
        }
    }
}

/// Returns `true` if any access in slice `a` conflicts with any access in
/// slice `b`.
pub fn has_conflicts(a: &[Access], b: &[Access]) -> bool {
    a.iter().any(|x| b.iter().any(|y| x.conflicts_with(y)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn type_id<T: 'static>() -> TypeId {
        TypeId::of::<T>()
    }

    #[test]
    fn read_read_same_type_no_conflict() {
        let a = Access::ResRead(type_id::<u32>());
        let b = Access::ResRead(type_id::<u32>());
        assert!(!a.conflicts_with(&b));
    }

    #[test]
    fn read_write_same_type_conflicts() {
        let a = Access::ResRead(type_id::<u32>());
        let b = Access::ResWrite(type_id::<u32>());
        assert!(a.conflicts_with(&b));
        // symmetric
        assert!(b.conflicts_with(&a));
    }

    #[test]
    fn write_write_same_type_conflicts() {
        let a = Access::ResWrite(type_id::<u32>());
        let b = Access::ResWrite(type_id::<u32>());
        assert!(a.conflicts_with(&b));
    }

    #[test]
    fn read_write_different_type_no_conflict() {
        let a = Access::ResRead(type_id::<u32>());
        let b = Access::ResWrite(type_id::<f32>());
        assert!(!a.conflicts_with(&b));
    }

    #[test]
    fn comp_read_write_conflicts() {
        let a = Access::CompRead(type_id::<u64>());
        let b = Access::CompWrite(type_id::<u64>());
        assert!(a.conflicts_with(&b));
        // symmetric
        assert!(b.conflicts_with(&a));
    }

    #[test]
    fn res_and_comp_same_type_no_conflict() {
        // Cross-namespace: ResWrite and CompWrite with the identical TypeId are
        // independent namespaces and must not conflict.
        let a = Access::ResWrite(type_id::<u32>());
        let b = Access::CompWrite(type_id::<u32>());
        assert!(!a.conflicts_with(&b));
    }

    #[test]
    fn has_conflicts_finds_conflict_in_sets() {
        let set_a = vec![
            Access::ResRead(type_id::<u32>()),
            Access::CompRead(type_id::<f32>()),
        ];
        let set_b = vec![
            Access::ResWrite(type_id::<u32>()), // conflicts with ResRead(u32) in set_a
            Access::CompRead(type_id::<f32>()),
        ];
        assert!(has_conflicts(&set_a, &set_b));
    }

    #[test]
    fn has_conflicts_empty_sets_no_conflict() {
        assert!(!has_conflicts(&[], &[]));
        assert!(!has_conflicts(&[Access::ResRead(type_id::<u32>())], &[]));
        assert!(!has_conflicts(&[], &[Access::ResWrite(type_id::<u32>())]));
    }
}
