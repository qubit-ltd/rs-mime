// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Shared admission count and asynchronous settlement notification.

use std::sync::Arc;
use std::sync::Mutex;
use std::task::Context;
use std::task::Poll;
use std::task::Waker;

use super::AdmissionPermit;
use super::AdmissionState;
use super::AdmissionsSettled;

/// Tracks callbacks that may still publish an operation observer.
///
/// The count and its waker queue share one mutex to make waiter registration
/// atomic with the transition to zero.
#[derive(Default)]
pub(in crate::detector::mime_execution_services) struct AdmissionTracker {
    /// Active admission count and tasks waiting for that count to reach zero.
    state: Mutex<AdmissionState>,
}

impl AdmissionTracker {
    /// Registers an admission and returns the guard that releases it.
    ///
    /// # Parameters
    /// - `self`: Shared tracker receiving the new active admission.
    ///
    /// # Returns
    /// A permit whose drop removes this admission from the active count.
    pub(in crate::detector::mime_execution_services) fn register(self: &Arc<Self>) -> AdmissionPermit {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.active += 1;
        drop(state);
        AdmissionPermit::new(Arc::clone(self))
    }

    /// Creates a future observing when the active count reaches zero.
    ///
    /// # Parameters
    /// - `self`: Tracker whose active count the future observes.
    ///
    /// # Returns
    /// A future that completes when all active admission permits are dropped.
    pub(in crate::detector::mime_execution_services) fn settled(self: &Arc<Self>) -> AdmissionsSettled {
        AdmissionsSettled::new(Arc::downgrade(self))
    }

    /// Checks whether an admission callback still holds a permit.
    ///
    /// # Parameters
    /// - `self`: Tracker whose active count is checked.
    ///
    /// # Returns
    /// `true` when the active count is nonzero.
    pub(in crate::detector::mime_execution_services) fn has_active(&self) -> bool {
        self.state.lock().unwrap_or_else(|error| error.into_inner()).active != 0
    }

    /// Checks the active count without waiting for the tracker mutex in tests.
    ///
    /// # Parameters
    /// - `self`: Tracker whose mutex availability is checked.
    ///
    /// # Returns
    /// `Some(true)` or `Some(false)` when the mutex is available, or `None`
    /// when a concurrent operation currently holds it.
    #[cfg(test)]
    pub(in crate::detector::mime_execution_services) fn try_has_active(&self) -> Option<bool> {
        self.state.try_lock().ok().map(|state| state.active != 0)
    }

    /// Releases one permit and wakes waiters if the active count reaches zero.
    ///
    /// # Parameters
    /// - `self`: Tracker whose active count is decremented.
    pub(in crate::detector::mime_execution_services) fn release(&self) {
        let waiters = {
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            state.active -= 1;
            if state.active == 0 {
                std::mem::take(&mut state.waiters)
            } else {
                Vec::new()
            }
        };
        for waiter in waiters {
            waiter.wake();
        }
    }

    /// Polls the zero-count condition and registers a task waker when needed.
    ///
    /// Waker cloning, disposal, and wake callbacks run outside the tracker
    /// mutex because a custom `RawWaker` can re-enter MIME services.
    ///
    /// # Parameters
    /// - `self`: Tracker whose count is checked.
    /// - `context`: Poll context supplying the waiter waker.
    ///
    /// # Returns
    /// `Ready(())` when there are no active admissions; otherwise `Pending`.
    pub(in crate::detector::mime_execution_services) fn poll_settled(&self, context: &Context<'_>) -> Poll<()> {
        let candidate = context.waker().clone();
        let (ready, unused) = {
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            if state.active == 0 {
                (true, Some(candidate))
            } else if state.waiters.iter().any(|waiter: &Waker| waiter.will_wake(&candidate)) {
                (false, Some(candidate))
            } else {
                state.waiters.push(candidate);
                (false, None)
            }
        };
        drop(unused);
        if ready { Poll::Ready(()) } else { Poll::Pending }
    }
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;
    use std::task::Context;
    use std::task::Poll;
    use std::task::Wake;
    use std::task::Waker;

    use super::AdmissionTracker;

    struct CountingWake(AtomicUsize);

    impl Wake for CountingWake {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }

        fn wake_by_ref(self: &Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn test_tracker_recovers_from_poison_and_deduplicates_waiters() {
        let tracker = Arc::new(AdmissionTracker::default());
        let poisoned = Arc::clone(&tracker);
        assert!(
            std::thread::spawn(move || {
                let _state = poisoned.state.lock().unwrap();
                panic!("poison admission tracker for recovery test");
            })
            .join()
            .is_err()
        );

        let permit = tracker.register();
        assert!(tracker.has_active());
        let wake = Arc::new(CountingWake(AtomicUsize::new(0)));
        let waker = Waker::from(Arc::clone(&wake));
        let mut context = Context::from_waker(&waker);
        let mut settled = Box::pin(tracker.settled());

        assert_eq!(Poll::Pending, settled.as_mut().poll(&mut context));
        assert_eq!(Poll::Pending, settled.as_mut().poll(&mut context));
        assert_eq!(
            1,
            tracker
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .waiters
                .len()
        );
        drop(permit);

        assert!(!tracker.has_active());
        assert_eq!(1, wake.0.load(Ordering::SeqCst));
        assert_eq!(Poll::Ready(()), settled.as_mut().poll(&mut context));
    }
}
