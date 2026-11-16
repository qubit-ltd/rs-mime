// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::Arc;

use super::Amount;
use super::MimeBudget;

/// Shared reservation state whose final drop returns bytes to the budget.
pub(super) struct PermitInner {
    /// Budget whose counter owns this reservation.
    pub(super) budget: Arc<MimeBudget>,
    /// Reserved resource category and amount.
    pub(super) amount: Amount,
}

impl Drop for PermitInner {
    /// Releases the reserved byte count when the final permit owner is dropped.
    fn drop(&mut self) {
        let mut counters = self.budget.counters.lock().unwrap_or_else(|e| e.into_inner());
        match self.amount {
            Amount::Owned(bytes) => counters.owned -= bytes,
            Amount::Staging(bytes) => counters.staging -= bytes,
        }
    }
}
