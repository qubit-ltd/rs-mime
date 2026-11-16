// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================

use std::sync::Arc;
use std::sync::Mutex;

use super::ReadCounters;

/// Cumulative read allowance shared by one root request and its descendants.
#[derive(Debug)]
pub(in crate::detector) struct ReadTreeBudget {
    pub(super) limit: u64,
    pub(super) counters: Mutex<ReadCounters>,
}

impl ReadTreeBudget {
    pub(in crate::detector) fn new(limit: u64) -> Arc<Self> {
        Arc::new(Self {
            limit,
            counters: Mutex::new(ReadCounters::default()),
        })
    }

    /// Reserves requested read capacity before provider I/O begins.
    #[allow(dead_code)]
    pub(in crate::detector) fn consumed(&self) -> u64 {
        self.counters
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .consumed
    }
}
