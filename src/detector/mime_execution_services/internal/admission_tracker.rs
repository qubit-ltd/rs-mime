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

use super::AdmissionPermit;
use super::AdmissionState;
use super::AdmissionsSettled;
use super::WaiterRegistration;
use crate::safe_wake::wake_safely;

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
            wake_safely(waiter.waker);
        }
    }

    /// Removes a future's waiter registration after it is dropped.
    ///
    /// # Parameters
    /// - `self`: Tracker whose waiter queue is updated.
    /// - `identity`: Stable identity of the dropped future.
    pub(in crate::detector::mime_execution_services) fn unregister_waiter(&self, identity: &Arc<()>) {
        let removed = {
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            state
                .waiters
                .iter()
                .position(|waiter| Arc::ptr_eq(&waiter.identity, identity))
                .map(|index| state.waiters.swap_remove(index))
        };
        drop(removed);
    }

    /// Polls the zero-count condition and registers a task waker when needed.
    ///
    /// Waker cloning, disposal, and wake callbacks run outside the tracker
    /// mutex because a custom `RawWaker` can re-enter MIME services.
    ///
    /// # Parameters
    /// - `self`: Tracker whose count is checked.
    /// - `identity`: Stable identity of the future registering its waiter.
    /// - `context`: Poll context supplying the waiter waker.
    ///
    /// # Returns
    /// `Ready(())` when there are no active admissions; otherwise `Pending`.
    pub(in crate::detector::mime_execution_services) fn poll_settled(
        &self,
        identity: &Arc<()>,
        context: &Context<'_>,
    ) -> Poll<()> {
        let candidate = context.waker().clone();
        let (ready, unused, replaced) = {
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            if state.active == 0 {
                let registered = state
                    .waiters
                    .iter()
                    .position(|waiter| Arc::ptr_eq(&waiter.identity, identity))
                    .map(|index| state.waiters.swap_remove(index));
                (true, Some(candidate), registered.map(|waiter| waiter.waker))
            } else if let Some(waiter) = state
                .waiters
                .iter_mut()
                .find(|waiter| Arc::ptr_eq(&waiter.identity, identity))
            {
                (false, None, Some(std::mem::replace(&mut waiter.waker, candidate)))
            } else {
                state.waiters.push(WaiterRegistration {
                    identity: Arc::clone(identity),
                    waker: candidate,
                });
                (false, None, None)
            }
        };
        drop(unused);
        drop(replaced);
        if ready { Poll::Ready(()) } else { Poll::Pending }
    }
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
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

    struct PanickingWake {
        dropped: Arc<AtomicBool>,
    }

    impl Wake for PanickingWake {
        fn wake(self: Arc<Self>) {
            std::panic::panic_any(PanicOnDrop(Arc::clone(&self.dropped)));
        }

        fn wake_by_ref(self: &Arc<Self>) {
            std::panic::panic_any(PanicOnDrop(Arc::clone(&self.dropped)));
        }
    }

    struct PanicOnDrop(Arc<AtomicBool>);

    impl Drop for PanicOnDrop {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
            panic!("panic payload destructor");
        }
    }

    #[test]
    fn test_panicking_settlement_waiter_does_not_block_other_waiters() {
        let tracker = Arc::new(AdmissionTracker::default());
        let permit = tracker.register();
        let payload_dropped = Arc::new(AtomicBool::new(false));
        let panicking_waker = Waker::from(Arc::new(PanickingWake {
            dropped: Arc::clone(&payload_dropped),
        }));
        let counting_wake = Arc::new(CountingWake(AtomicUsize::new(0)));
        let counting_waker = Waker::from(Arc::clone(&counting_wake));
        let mut panicking_context = Context::from_waker(&panicking_waker);
        let mut counting_context = Context::from_waker(&counting_waker);
        let mut panicking_waiter = Box::pin(tracker.settled());
        let mut counting_waiter = Box::pin(tracker.settled());

        assert_eq!(Poll::Pending, panicking_waiter.as_mut().poll(&mut panicking_context));
        assert_eq!(Poll::Pending, counting_waiter.as_mut().poll(&mut counting_context));
        drop(permit);

        assert!(payload_dropped.load(Ordering::SeqCst));
        assert_eq!(1, counting_wake.0.load(Ordering::SeqCst));
        assert!(!tracker.has_active());
        assert_eq!(Poll::Ready(()), panicking_waiter.as_mut().poll(&mut panicking_context));
        assert_eq!(Poll::Ready(()), counting_waiter.as_mut().poll(&mut counting_context));
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

    #[test]
    fn test_cancelled_settlement_waiters_unregister_their_wakers() {
        let tracker = Arc::new(AdmissionTracker::default());
        let _permit = tracker.register();
        let wake = Arc::new(CountingWake(AtomicUsize::new(0)));
        let waker = Waker::from(Arc::clone(&wake));
        let mut context = Context::from_waker(&waker);

        for _ in 0..16 {
            let mut settled = Box::pin(tracker.settled());
            assert_eq!(Poll::Pending, settled.as_mut().poll(&mut context));
            assert_eq!(3, Arc::strong_count(&wake));
            drop(settled);
            assert_eq!(2, Arc::strong_count(&wake));
            assert!(
                tracker
                    .state
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .waiters
                    .is_empty()
            );
        }
    }

    #[test]
    fn test_settlement_waiter_replaces_waker_and_wakes_concurrent_waiters() {
        let tracker = Arc::new(AdmissionTracker::default());
        let first_permit = tracker.register();
        let second_permit = tracker.register();
        let first_wake = Arc::new(CountingWake(AtomicUsize::new(0)));
        let second_wake = Arc::new(CountingWake(AtomicUsize::new(0)));
        let first_waker = Waker::from(Arc::clone(&first_wake));
        let second_waker = Waker::from(Arc::clone(&second_wake));
        let mut first_context = Context::from_waker(&first_waker);
        let mut second_context = Context::from_waker(&second_waker);
        let mut first = Box::pin(tracker.settled());
        let mut second = Box::pin(tracker.settled());

        assert_eq!(Poll::Pending, first.as_mut().poll(&mut first_context));
        assert_eq!(Poll::Pending, first.as_mut().poll(&mut second_context));
        assert_eq!(2, Arc::strong_count(&first_wake));
        assert_eq!(3, Arc::strong_count(&second_wake));
        assert_eq!(Poll::Pending, second.as_mut().poll(&mut first_context));
        assert_eq!(Poll::Pending, second.as_mut().poll(&mut second_context));
        assert_eq!(2, tracker.state.lock().unwrap().waiters.len());

        drop(first_permit);
        assert_eq!(0, first_wake.0.load(Ordering::SeqCst));
        drop(second_permit);
        assert_eq!(2, second_wake.0.load(Ordering::SeqCst));
        assert_eq!(0, first_wake.0.load(Ordering::SeqCst));
        assert_eq!(Poll::Ready(()), first.as_mut().poll(&mut second_context));
        assert_eq!(Poll::Ready(()), second.as_mut().poll(&mut second_context));
    }
}
