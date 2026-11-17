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
    #[must_use = "the read budget must be retained for its operation scopes"]
    pub(in crate::detector) fn new(limit: u64) -> Arc<Self> {
        Arc::new(Self {
            limit,
            counters: Mutex::new(ReadCounters::default()),
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
