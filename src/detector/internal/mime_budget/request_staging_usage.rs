// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::Arc;
use std::sync::Mutex;

#[derive(Debug, Default)]
struct Counters {
    current: u64,
    peak: u64,
}

/// Tracks simultaneous staging reservations belonging to one request.
#[derive(Debug, Default)]
pub(crate) struct RequestStagingUsage {
    counters: Mutex<Counters>,
}

impl RequestStagingUsage {
    /// Returns the peak simultaneously reserved staging bytes.
    pub(crate) fn peak(&self) -> u64 {
        self.counters.lock().unwrap_or_else(|error| error.into_inner()).peak
    }

    /// Returns bytes currently held by staging permits.
    #[cfg(test)]
    pub(super) fn current(&self) -> u64 {
        self.counters.lock().unwrap_or_else(|error| error.into_inner()).current
    }

    #[cfg(test)]
    pub(super) fn poison_lock_for_test(&self) {
        let _ = std::panic::catch_unwind(|| {
            let _counters = self.counters.lock().unwrap();
            panic!("poison request staging counters");
        });
    }

    /// Records a reservation and returns its RAII accounting guard.
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
}

/// Releases current request usage when the corresponding staging permit ends.
pub(crate) struct RequestStagingGuard {
    usage: Arc<RequestStagingUsage>,
    bytes: u64,
}

impl Drop for RequestStagingGuard {
    fn drop(&mut self) {
        let mut counters = self.usage.counters.lock().unwrap_or_else(|error| error.into_inner());
        counters.current = counters
            .current
            .checked_sub(self.bytes)
            .expect("staging usage guard underflow");
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::RequestStagingUsage;

    #[test]
    fn request_staging_overlapping_reservations_track_peak_and_release_current() {
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
    fn request_staging_sequential_reservations_keep_largest_peak() {
        let usage = Arc::new(RequestStagingUsage::default());
        drop(usage.reserve(4).unwrap());
        drop(usage.reserve(3).unwrap());
        assert_eq!((usage.current(), usage.peak()), (0, 4));
    }

    #[test]
    fn request_staging_trackers_are_independent_and_overflow_rolls_back() {
        let first = Arc::new(RequestStagingUsage::default());
        let second = Arc::new(RequestStagingUsage::default());
        let _permit = first.reserve(u64::MAX).unwrap();
        assert!(first.reserve(1).is_none());
        assert_eq!((first.current(), first.peak()), (u64::MAX, u64::MAX));
        assert_eq!((second.current(), second.peak()), (0, 0));
    }
}
