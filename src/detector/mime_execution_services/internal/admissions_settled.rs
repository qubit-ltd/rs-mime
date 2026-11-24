// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Future that observes the admission tracker's zero-count notification.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Weak;
use std::task::Context;
use std::task::Poll;

use super::AdmissionTracker;

/// Resolves when the tracker has no in-flight admission permits.
pub(in crate::detector::mime_execution_services) struct AdmissionsSettled {
    /// Tracker observed without extending its lifetime.
    tracker: Weak<AdmissionTracker>,
    /// Stable identity used to replace and unregister this future's waiter.
    identity: Arc<()>,
}

impl AdmissionsSettled {
    /// Creates a waiter for the supplied admission tracker.
    ///
    /// # Parameters
    /// - `tracker`: Shared tracker whose active count will be observed.
    ///
    /// # Returns
    /// A future that can be polled for the zero-admission condition.
    pub(in crate::detector::mime_execution_services) fn new(
        tracker: Weak<AdmissionTracker>,
    ) -> Self {
        Self {
            tracker,
            identity: Arc::new(()),
        }
    }
}

impl Future for AdmissionsSettled {
    /// Unit is returned once admission tracking becomes idle.
    type Output = ();

    /// Checks the active admission count and registers the current task if
    /// needed.
    ///
    /// # Parameters
    /// - `self`: Waiter being polled.
    /// - `context`: Poll context containing the task's waker.
    ///
    /// # Returns
    /// `Ready(())` when no tracker remains or the count reaches zero; otherwise
    /// `Pending` until an admission guard wakes this task.
    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let Some(tracker) = self.tracker.upgrade() else {
            return Poll::Ready(());
        };
        tracker.poll_settled(&self.identity, context)
    }
}

impl Drop for AdmissionsSettled {
    fn drop(&mut self) {
        if let Some(tracker) = self.tracker.upgrade() {
            tracker.unregister_waiter(&self.identity);
        }
    }
}
