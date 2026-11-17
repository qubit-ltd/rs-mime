// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::Arc;
use std::sync::Mutex;

use super::ReadCounters;

/// Cumulative read allowance shared by one root request and its descendants.
#[derive(Debug)]
pub(in crate::detector) struct ReadTreeBudget {
    /// Maximum bytes shared by the root operation and all descendants.
    pub(super) limit: u64,
    /// Committed and reserved bytes charged to the shared tree.
    pub(super) counters: Mutex<ReadCounters>,
}

impl ReadTreeBudget {
    /// Creates an empty read budget shared by related operation scopes.
    ///
    /// # Parameters
    /// - `limit`: Maximum bytes that the tree may consume.
    ///
    /// # Returns
    /// A reference-counted read budget.
    pub(in crate::detector) fn new(limit: u64) -> Arc<Self> {
        Arc::new(Self {
            limit,
            counters: Mutex::new(ReadCounters::default()),
        })
    }

    /// Returns bytes committed across the shared read tree.
    #[allow(dead_code)]
    pub(in crate::detector) fn consumed(&self) -> u64 {
        self.counters.lock().unwrap_or_else(|e| e.into_inner()).consumed
    }
}

#[cfg(test)]
mod tests {
    use super::ReadTreeBudget;

    #[test]
    fn test_consumed_recovers_poisoned_counters() {
        let budget = ReadTreeBudget::new(8);
        let counters = std::sync::Arc::clone(&budget);
        assert!(
            std::thread::spawn(move || {
                let _guard = counters.counters.lock().unwrap();
                panic!("poison read-tree counters");
            })
            .join()
            .is_err()
        );

        assert_eq!(0, budget.consumed());
    }
}
