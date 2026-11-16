// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::Arc;

use super::MimeBudget;
use super::OwnedMemoryPermit;
use super::StagingPermit;

/// Opaque cloneable access to the service-wide staging budget.
#[derive(Clone)]
pub(crate) struct StagingBudget {
    /// Shared counters and limits for owned and staged bytes.
    budget: Arc<MimeBudget>,
}

impl StagingBudget {
    /// Creates a staging-budget handle backed by a shared MIME budget.
    ///
    /// # Parameters
    /// - `budget`: Shared budget that tracks staging reservations.
    ///
    /// # Returns
    /// A cloneable staging-budget handle.
    pub(in crate::detector) fn from_budget(budget: Arc<MimeBudget>) -> Self {
        Self { budget }
    }

    /// Attempts to reserve staging capacity for a resource.
    ///
    /// # Parameters
    /// - `bytes`: Number of staging bytes requested.
    ///
    /// # Returns
    /// A permit when capacity is available, or `None` if the limit would be
    /// exceeded.
    pub(crate) fn try_reserve(&self, bytes: u64) -> Option<StagingPermit> {
        self.budget
            .reserve_staging(bytes)
            .map(|permit| StagingPermit { _permit: permit })
    }

    /// Attempts to reserve owned memory for a copied staging chunk.
    ///
    /// # Parameters
    /// - `bytes`: Number of owned-memory bytes requested.
    ///
    /// # Returns
    /// A permit when capacity is available, or `None` if the limit would be
    /// exceeded.
    pub(crate) fn try_reserve_owned(&self, bytes: usize) -> Option<OwnedMemoryPermit> {
        self.budget
            .reserve_owned(bytes)
            .map(|permit| OwnedMemoryPermit { _permit: permit })
    }

    /// Returns the configured maximum staging size.
    pub(crate) fn limit(&self) -> u64 {
        self.budget.limits.staging
    }

    /// Returns currently reserved staging bytes for budget assertions.
    #[cfg(test)]
    pub(crate) fn used_bytes(&self) -> u64 {
        self.budget.staging_bytes()
    }
}
