// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! RAII guard that keeps one MIME admission visible to shutdown.

use std::sync::Arc;

use super::AdmissionTracker;

/// Releases one in-flight admission when the submission path exits.
pub(in crate::detector::mime_execution_services) struct AdmissionPermit {
    /// Tracker whose active count this permit owns.
    tracker: Arc<AdmissionTracker>,
}

impl AdmissionPermit {
    /// Creates a permit for one already-registered admission.
    ///
    /// # Parameters
    /// - `tracker`: Shared tracker containing the admission count and waiters.
    ///
    /// # Returns
    /// A guard that releases this admission when dropped.
    pub(in crate::detector::mime_execution_services) fn new(tracker: Arc<AdmissionTracker>) -> Self {
        Self { tracker }
    }
}

impl Drop for AdmissionPermit {
    /// Decrements the active count and wakes settlement waiters after unlocking.
    ///
    /// # Parameters
    /// - `self`: Admission guard being released.
    fn drop(&mut self) {
        self.tracker.release();
    }
}
