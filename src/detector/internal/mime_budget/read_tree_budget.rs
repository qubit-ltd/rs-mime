// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use super::ReadCounters;

/// Cumulative read allowance shared by one root request and its descendants.
#[derive(Debug)]
pub(in crate::detector) struct ReadTreeBudget {
    /// Maximum bytes shared by the root operation and all descendants.
    pub(super) limit: AtomicU64,
    /// Committed and reserved bytes charged to the shared tree.
    pub(super) counters: Mutex<ReadCounters>,
    /// Serializes the first real binding with reads from borrowed contexts.
    pub(super) binding: Mutex<()>,
}

impl ReadTreeBudget {
    /// Creates an empty read budget shared by related operation scopes.
    ///
    /// # Parameters
    /// - `limit`: Maximum bytes that the tree may consume.
    ///
    /// # Returns
    /// A reference-counted read budget.
    #[must_use = "the read budget must be retained for its operation scopes"]
    pub(in crate::detector) fn new(limit: u64) -> Arc<Self> {
        Arc::new(Self {
            limit: AtomicU64::new(limit),
            counters: Mutex::new(ReadCounters::default()),
            binding: Mutex::new(()),
        })
    }

    /// Returns bytes committed across the shared read tree.
    ///
    /// # Returns
    /// The total committed byte count.
    #[must_use]
    #[allow(dead_code)]
    pub(in crate::detector) fn consumed(&self) -> u64 {
        self.counters.lock().unwrap_or_else(|e| e.into_inner()).consumed
    }

    /// Selects the cumulative limit before the first read is reserved.
    ///
    /// # Parameters
    /// - `limit`: Maximum cumulative bytes allowed in this tree.
    pub(in crate::detector) fn select_limit_before_reads(&self, limit: u64) {
        let counters = self.counters.lock().unwrap_or_else(|e| e.into_inner());
        if counters.consumed == 0 && counters.reserved == 0 {
            self.limit.store(limit, Ordering::Release);
        }
    }

    /// Returns the cumulative allowance selected for this tree.
    pub(in crate::detector) fn limit(&self) -> u64 {
        self.limit.load(Ordering::Acquire)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::thread;

    use super::ReadTreeBudget;

    #[test]
    fn test_consumed_recovers_poisoned_counters() {
        let budget = ReadTreeBudget::new(8);
        let counters = Arc::clone(&budget);
        assert!(
            thread::spawn(move || {
                let _guard = counters.counters.lock().unwrap();
                panic!("poison read-tree counters");
            })
            .join()
            .is_err()
        );

        assert_eq!(0, budget.consumed());
    }
}
