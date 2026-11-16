// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================

use std::sync::Arc;

use super::ReadScopeBudget;
use super::ReadTreeBudget;

pub(crate) struct ReadPermit {
    pub(super) tree: Arc<ReadTreeBudget>,
    pub(super) scopes: Vec<Arc<ReadScopeBudget>>,
    pub(super) reserved: u64,
    pub(super) committed: bool,
}

impl ReadPermit {
    /// Commits the bytes actually returned and releases unused capacity.
    pub(crate) fn commit(mut self, actual: u64) -> u64 {
        assert!(
            actual <= self.reserved,
            "provider returned more bytes than reserved"
        );
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
    fn drop(&mut self) {
        if !self.committed {
            self.tree
                .counters
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .reserved -= self.reserved;
            for scope in &self.scopes {
                scope
                    .counters
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .reserved -= self.reserved;
            }
        }
    }
}
