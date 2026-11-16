// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================

use std::sync::Arc;

use super::Amount;
use super::MimeBudget;

pub(super) struct PermitInner {
    pub(super) budget: Arc<MimeBudget>,
    pub(super) amount: Amount,
}

impl Drop for PermitInner {
    fn drop(&mut self) {
        let mut counters = self
            .budget
            .counters
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        match self.amount {
            Amount::Owned(bytes) => counters.owned -= bytes,
            Amount::Staging(bytes) => counters.staging -= bytes,
        }
    }
}
