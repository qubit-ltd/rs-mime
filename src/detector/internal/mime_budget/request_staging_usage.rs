// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#[cfg(test)]
use std::panic::catch_unwind;
use std::sync::Arc;
use std::sync::Mutex;

use super::request_staging_counters::RequestStagingCounters;
pub(crate) use super::request_staging_guard::RequestStagingGuard;

/// Tracks simultaneous staging reservations belonging to one request.
#[derive(Debug, Default)]
pub(crate) struct RequestStagingUsage {
    /// Shared counters for reservations held by this request.
    pub(super) counters: Mutex<RequestStagingCounters>,
}

impl RequestStagingUsage {
    /// Returns the peak simultaneously reserved staging bytes.
    ///
    /// # Returns
    /// The highest simultaneous staging byte count observed for this request.
    #[must_use]
    #[inline]
    pub(crate) fn peak(&self) -> u64 {
        self.counters.lock().unwrap_or_else(|error| error.into_inner()).peak
    }

    /// Records a reservation and returns its RAII accounting guard.
    ///
    /// # Parameters
    /// - `bytes`: Number of staging bytes to reserve.
    ///
    /// # Returns
    /// `Some` with a guard that releases `bytes` when dropped, or `None` if
    /// adding `bytes` would overflow the current count.
    pub(crate) fn reserve(self: &Arc<Self>, bytes: u64) -> Option<RequestStagingGuard> {
        let mut counters = self.counters.lock().unwrap_or_else(|error| error.into_inner());
        let current = counters.current.checked_add(bytes)?;
        counters.current = current;
        counters.peak = counters.peak.max(current);
        Some(RequestStagingGuard {
            usage: Arc::clone(self),
            bytes,
        })
    }

    /// Returns bytes currently held by staging permits.
    #[cfg(test)]
    pub(super) fn current(&self) -> u64 {
        self.counters.lock().unwrap_or_else(|error| error.into_inner()).current
    }

    #[cfg(test)]
    pub(super) fn poison_lock_for_test(&self) {
        let _ = catch_unwind(|| {
            let _counters = self.counters.lock().unwrap();
            panic!("poison request staging counters");
        });
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::RequestStagingUsage;

    #[test]
    fn test_request_staging_overlapping_reservations_track_peak_and_release_current() {
        let usage = Arc::new(RequestStagingUsage::default());
        let first = usage.reserve(4).unwrap();
        assert_eq!((usage.current(), usage.peak()), (4, 4));
        let second = usage.reserve(3).unwrap();
        assert_eq!((usage.current(), usage.peak()), (7, 7));
        drop(first);
        assert_eq!((usage.current(), usage.peak()), (3, 7));
        drop(second);
        assert_eq!((usage.current(), usage.peak()), (0, 7));
    }

    #[test]
    fn test_request_staging_sequential_reservations_keep_largest_peak() {
        let usage = Arc::new(RequestStagingUsage::default());
        drop(usage.reserve(4).unwrap());
        drop(usage.reserve(3).unwrap());
        assert_eq!((usage.current(), usage.peak()), (0, 4));
    }

    #[test]
    fn test_request_staging_trackers_are_independent_and_overflow_rolls_back() {
        let first = Arc::new(RequestStagingUsage::default());
        let second = Arc::new(RequestStagingUsage::default());
        let _permit = first.reserve(u64::MAX).unwrap();
        assert!(first.reserve(1).is_none());
        assert_eq!((first.current(), first.peak()), (u64::MAX, u64::MAX));
        assert_eq!((second.current(), second.peak()), (0, 0));
    }
}
