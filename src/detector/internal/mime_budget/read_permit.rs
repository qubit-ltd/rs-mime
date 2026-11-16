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
