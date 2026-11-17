// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::Arc;

use super::ReadScopeBudget;
use super::ReadTreeBudget;

/// Provisional read reservation charged to a tree and its ancestor scopes.
#[must_use = "the read reservation must be committed or held until the read completes"]
pub(crate) struct ReadPermit {
    /// Shared root budget charged when this reservation is committed.
    pub(super) tree: Arc<ReadTreeBudget>,
    /// Scopes charged for this read, from the current scope through its roots.
    pub(super) scopes: Vec<Arc<ReadScopeBudget>>,
    /// Capacity provisionally reserved before provider I/O.
    pub(super) reserved: u64,
    /// Whether completion has replaced the reservation with actual bytes.
    pub(super) committed: bool,
}

impl ReadPermit {
    /// Commits the bytes actually returned and releases unused capacity.
    ///
    /// # Parameters
    /// - `actual`: Number of bytes the provider actually returned.
    ///
    /// # Returns
    /// The committed byte count.
    ///
    /// # Panics
    /// Panics if the provider returned more bytes than were reserved.
    pub(crate) fn commit(mut self, actual: u64) -> u64 {
        assert!(actual <= self.reserved, "provider returned more bytes than reserved");
        let mut counters = self.tree.counters.lock().unwrap_or_else(|e| e.into_inner());
        counters.reserved -= self.reserved;
        counters.consumed += actual;
        for scope in &self.scopes {
            let mut counters = scope.counters.lock().unwrap_or_else(|e| e.into_inner());
            counters.reserved -= self.reserved;
            counters.consumed += actual;
        }
        self.committed = true;
        actual
    }
}

impl Drop for ReadPermit {
    /// Releases provisional bytes when the read did not commit.
    fn drop(&mut self) {
        if !self.committed {
            self.tree.counters.lock().unwrap_or_else(|e| e.into_inner()).reserved -= self.reserved;
            for scope in &self.scopes {
                scope.counters.lock().unwrap_or_else(|e| e.into_inner()).reserved -= self.reserved;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::panic::AssertUnwindSafe;
    use std::panic::catch_unwind;
    use std::sync::Arc;
    use std::thread;

    use super::super::ReadTreeContext;

    #[test]
    fn test_short_read_commit_charges_actual_bytes_to_all_ancestors() {
        let root = ReadTreeContext::new(10);
        let child = root.child(7);
        let grandchild = child.child(5);
        let permit = grandchild.reserve(4).expect("reservation fits each scope");
        assert_eq!(3, permit.commit(3));
        assert_eq!(3, root.consumed());
        assert_eq!(3, child.consumed());
        assert_eq!(3, grandchild.consumed());
        assert_eq!(7, root.remaining());
        assert_eq!(4, child.remaining());
        assert_eq!(2, grandchild.remaining());
    }

    #[test]
    fn test_overreported_read_panics_and_rolls_back_reservation() {
        let root = ReadTreeContext::new(8);
        let child = root.child(6);
        let permit = child.reserve(4).expect("four bytes fit both budgets");
        assert_eq!(root.remaining(), 4);
        assert_eq!(child.remaining(), 2);

        let result = catch_unwind(AssertUnwindSafe(|| permit.commit(5)));
        assert!(result.is_err(), "a read cannot commit more than it reserved");
        assert_eq!(root.consumed(), 0);
        assert_eq!(child.consumed(), 0);
        assert_eq!(root.remaining(), 8);
        assert_eq!(child.remaining(), 6);
    }

    #[test]
    fn test_dropping_uncommitted_permit_recovers_poisoned_tree_and_scope_locks() {
        let root = ReadTreeContext::new(10);
        let child = root.child(5);
        let permit = child.reserve(4).expect("reservation fits both scopes");
        let tree = Arc::clone(&permit.tree);
        let scope = Arc::clone(&permit.scopes[0]);

        assert!(
            thread::spawn(move || {
                let _guard = tree.counters.lock().unwrap();
                panic!("poison shared read-tree counters");
            })
            .join()
            .is_err()
        );
        assert!(
            thread::spawn(move || {
                let _guard = scope.counters.lock().unwrap();
                panic!("poison child read-scope counters");
            })
            .join()
            .is_err()
        );

        drop(permit);
        assert_eq!(10, root.remaining());
        assert_eq!(5, child.remaining());
    }
}
