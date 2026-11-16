// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================

use std::sync::Arc;

use super::MimeBudget;
use super::OwnedMemoryPermit;
use super::StagingPermit;

/// Opaque cloneable access to the service-wide staging budget.
#[derive(Clone)]
pub(crate) struct StagingBudget {
    budget: Arc<MimeBudget>,
}

impl StagingBudget {
    pub(in crate::detector) fn from_budget(budget: Arc<MimeBudget>) -> Self {
        Self { budget }
    }

    pub(crate) fn try_reserve(&self, bytes: u64) -> Option<StagingPermit> {
        self.budget
            .reserve_staging(bytes)
            .map(|permit| StagingPermit { _permit: permit })
    }

    pub(crate) fn try_reserve_owned(&self, bytes: usize) -> Option<OwnedMemoryPermit> {
        self.budget
            .reserve_owned(bytes)
            .map(|permit| OwnedMemoryPermit { _permit: permit })
    }

    pub(crate) fn limit(&self) -> u64 {
        self.budget.limits.staging
    }

    #[cfg(test)]
    pub(crate) fn used_bytes(&self) -> u64 {
        self.budget.staging_bytes()
    }
}
