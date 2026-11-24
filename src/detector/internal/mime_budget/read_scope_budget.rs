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

use super::ScopeCounters;

/// Per-scope read allowance linked to ancestor scopes.
#[derive(Debug)]
pub(super) struct ReadScopeBudget {
    /// Maximum committed plus reserved bytes for this scope subtree.
    pub(super) limit: u64,
    /// Committed and in-flight byte totals for this scope.
    pub(super) counters: Mutex<ScopeCounters>,
    /// Bytes committed by reads initiated in this exact operation scope.
    pub(super) local_consumed: AtomicU64,
    /// Parent scope charged for reads performed by this scope.
    pub(super) parent: Option<Arc<ReadScopeBudget>>,
}
