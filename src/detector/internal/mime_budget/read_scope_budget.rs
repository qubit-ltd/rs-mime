// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================

use std::sync::Arc;
use std::sync::Mutex;

use super::ScopeCounters;

#[derive(Debug)]
pub(super) struct ReadScopeBudget {
    pub(super) limit: u64,
    pub(super) counters: Mutex<ScopeCounters>,
    pub(super) parent: Option<Arc<ReadScopeBudget>>,
}
