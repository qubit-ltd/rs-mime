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
use super::RequestStagingUsage;
use super::StagingPermit;

/// Opaque cloneable access to the service-wide staging budget.
#[derive(Clone)]
pub(crate) struct StagingBudget {
    /// Shared counters and limits for owned and staged bytes.
    budget: Arc<MimeBudget>,
    /// Optional request-local staging usage tracker.
    usage: Option<Arc<RequestStagingUsage>>,
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
        Self {
            budget,
            usage: None,
        }
    }

    /// Returns a handle that also records permits in the given request.
    pub(crate) fn with_usage(mut self, usage: Arc<RequestStagingUsage>) -> Self {
        self.usage = Some(usage);
        self
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
        let permit = self.budget.reserve_staging(bytes)?;
        let usage = match &self.usage {
            Some(usage) => Some(usage.reserve(bytes)?),
            None => None,
        };
        Some(StagingPermit {
            _permit: permit,
            _usage: usage,
        })
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::super::Limits;
    use super::super::MimeBudget;
    use super::super::RequestStagingUsage;
    use super::StagingBudget;

    #[test]
    fn staging_budget_tracks_usage_and_releases_it_with_the_permit() {
        let budget = Arc::new(MimeBudget::new(Limits {
            owned: 0,
            staging: 7,
        }));
        let usage = Arc::new(RequestStagingUsage::default());
        let staging =
            StagingBudget::from_budget(Arc::clone(&budget)).with_usage(Arc::clone(&usage));
        let first = staging.try_reserve(4).unwrap();
        let second = staging.try_reserve(3).unwrap();
        assert_eq!((usage.current(), usage.peak()), (7, 7));
        drop((first, second));
        assert_eq!((usage.current(), usage.peak()), (0, 7));
        assert_eq!(budget.staging_bytes(), 0);
    }

    #[test]
    fn staging_budget_rejection_does_not_pollute_usage_or_global_counters() {
        let budget = Arc::new(MimeBudget::new(Limits {
            owned: 0,
            staging: 5,
        }));
        let usage = Arc::new(RequestStagingUsage::default());
        let staging =
            StagingBudget::from_budget(Arc::clone(&budget)).with_usage(Arc::clone(&usage));
        let permit = staging.try_reserve(5).unwrap();
        assert!(staging.try_reserve(1).is_none());
        assert_eq!((usage.current(), usage.peak()), (5, 5));
        drop(permit);
        assert_eq!((usage.current(), usage.peak()), (0, 5));
        assert_eq!(budget.staging_bytes(), 0);
    }

    #[test]
    fn staging_budget_without_tracker_keeps_global_limit_and_overflow_rolls_back() {
        let budget = Arc::new(MimeBudget::new(Limits {
            owned: 0,
            staging: 6,
        }));
        let staging = StagingBudget::from_budget(Arc::clone(&budget));
        let permit = staging.try_reserve(6).unwrap();
        assert!(staging.try_reserve(1).is_none());
        drop(permit);
        assert_eq!(budget.staging_bytes(), 0);

        let huge = Arc::new(MimeBudget::new(Limits {
            owned: 0,
            staging: u64::MAX,
        }));
        let usage = Arc::new(RequestStagingUsage::default());
        let tracked = StagingBudget::from_budget(Arc::clone(&huge)).with_usage(Arc::clone(&usage));
        let permit = tracked.try_reserve(u64::MAX).unwrap();
        assert!(tracked.try_reserve(1).is_none());
        drop(permit);
        assert_eq!((usage.current(), usage.peak()), (0, u64::MAX));
        assert_eq!(huge.staging_bytes(), 0);
    }

    #[test]
    fn staging_budget_recovers_poisoned_usage_lock() {
        let usage = Arc::new(RequestStagingUsage::default());
        usage.poison_lock_for_test();
        let permit = usage.reserve(2).unwrap();
        assert_eq!((usage.current(), usage.peak()), (2, 2));
        drop(permit);
        assert_eq!((usage.current(), usage.peak()), (0, 2));
    }
}
