// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

//! Receiver half of a completion channel.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;
use std::task::Context;
use std::task::Poll;

use super::Completion;

/// Receives a result published by its completion sender.
///
/// # Type Parameters
/// - `T`: Value delivered by the sender.
pub(in crate::managed_temp_staging) struct CompletionFuture<T>(
    #[doc = "Shared result and receiver-waker state."] pub(in crate::managed_temp_staging) Arc<Mutex<Completion<T>>>,
);

impl<T> Future for CompletionFuture<T> {
    /// Value delivered by the sender.
    type Output = T;

    /// Polls for a published value and registers the current task when pending.
    ///
    /// # Parameters
    /// - `self`: Receiver state being polled.
    /// - `context`: Current task context used to register a wakeup.
    ///
    /// # Returns
    /// `Ready(value)` after publication or `Pending` while awaiting a value.
    ///
    /// # Type Parameters
    /// - `T`: Value delivered by this receiver.
    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<T> {
        let mut candidate = Some(context.waker().clone());
        let (value, old_waker) = {
            let mut state = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
            if let Some(value) = state.value.take() {
                (Some(value), state.waker.take())
            } else {
                (None, std::mem::replace(&mut state.waker, candidate.take()))
            }
        };
        drop(old_waker);
        drop(candidate);
        match value {
            Some(value) => Poll::Ready(value),
            None => Poll::Pending,
        }
    }
}

impl<T> Drop for CompletionFuture<T> {
    fn drop(&mut self) {
        let waker = {
            let mut state = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
            state.waker.take()
        };
        drop(waker);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::task::Context;
    use std::task::Poll;
    use std::task::RawWaker;
    use std::task::RawWakerVTable;
    use std::task::Wake;
    use std::task::Waker;

    use super::super::completion_pair;

    struct WakeCounter(AtomicUsize);
    impl Wake for WakeCounter {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    struct LockProbe {
        state: Arc<Mutex<super::super::Completion<usize>>>,
        blocked: AtomicUsize,
    }

    impl LockProbe {
        fn inspect(&self) {
            if self.state.try_lock().is_err() {
                self.blocked.fetch_add(1, Ordering::SeqCst);
            }
        }
    }

    unsafe fn probe_clone(data: *const ()) -> RawWaker {
        // SAFETY: this callback owns one reference supplied by the raw waker.
        let probe = unsafe { Arc::<LockProbe>::from_raw(data.cast()) };
        probe.inspect();
        let clone = Arc::clone(&probe);
        std::mem::forget(probe);
        RawWaker::new(Arc::into_raw(clone).cast(), &PROBE_VTABLE)
    }

    unsafe fn probe_wake(data: *const ()) {
        // SAFETY: this callback consumes one reference supplied by the raw waker.
        let probe = unsafe { Arc::<LockProbe>::from_raw(data.cast()) };
        probe.inspect();
    }

    unsafe fn probe_wake_by_ref(data: *const ()) {
        // SAFETY: this callback borrows its reference and restores ownership below.
        let probe = unsafe { Arc::<LockProbe>::from_raw(data.cast()) };
        probe.inspect();
        std::mem::forget(probe);
    }

    unsafe fn probe_drop(data: *const ()) {
        // SAFETY: this callback consumes one reference supplied by the raw waker.
        let probe = unsafe { Arc::<LockProbe>::from_raw(data.cast()) };
        probe.inspect();
    }

    static PROBE_VTABLE: RawWakerVTable =
        RawWakerVTable::new(probe_clone, probe_wake, probe_wake_by_ref, probe_drop);

    fn lock_probe_waker(
        state: &Arc<Mutex<super::super::Completion<usize>>>,
    ) -> (Waker, Arc<LockProbe>) {
        let probe = Arc::new(LockProbe {
            state: Arc::clone(state),
            blocked: AtomicUsize::new(0),
        });
        let raw = RawWaker::new(Arc::into_raw(Arc::clone(&probe)).cast(), &PROBE_VTABLE);
        // SAFETY: each vtable operation reconstructs exactly one Arc reference.
        (unsafe { Waker::from_raw(raw) }, probe)
    }

    #[test]
    fn test_receiver_wakes_after_a_waiting_poll_and_takes_the_published_value() {
        let (sender, receiver) = completion_pair();
        let mut receiver = Box::pin(receiver);
        let wake_counter = Arc::new(WakeCounter(AtomicUsize::new(0)));
        let waker = Waker::from(Arc::clone(&wake_counter));
        let mut context = Context::from_waker(&waker);

        assert!(matches!(receiver.as_mut().poll(&mut context), Poll::Pending));
        assert_eq!(0, wake_counter.0.load(Ordering::SeqCst));
        sender.complete("worker result");
        assert_eq!(1, wake_counter.0.load(Ordering::SeqCst));
        assert!(matches!(
            receiver.as_mut().poll(&mut context),
            Poll::Ready("worker result")
        ));
    }

    #[test]
    fn test_receiver_takes_a_prepublished_value_once() {
        let (sender, receiver) = completion_pair();
        sender.complete("already available");
        let mut receiver = Box::pin(receiver);
        let waker = Waker::from(Arc::new(WakeCounter(AtomicUsize::new(0))));
        let mut context = Context::from_waker(&waker);

        assert!(matches!(
            receiver.as_mut().poll(&mut context),
            Poll::Ready("already available")
        ));
        assert!(matches!(receiver.as_mut().poll(&mut context), Poll::Pending));
    }

    #[test]
    fn test_receiver_replaces_stale_waker_with_latest_polling_task() {
        let (sender, receiver) = completion_pair();
        let mut receiver = Box::pin(receiver);
        let first_counter = Arc::new(WakeCounter(AtomicUsize::new(0)));
        let first_waker = Waker::from(Arc::clone(&first_counter));
        let mut first_context = Context::from_waker(&first_waker);
        assert!(matches!(receiver.as_mut().poll(&mut first_context), Poll::Pending));

        let latest_counter = Arc::new(WakeCounter(AtomicUsize::new(0)));
        let latest_waker = Waker::from(Arc::clone(&latest_counter));
        let mut latest_context = Context::from_waker(&latest_waker);
        assert!(matches!(receiver.as_mut().poll(&mut latest_context), Poll::Pending));

        sender.complete("latest waiter");
        assert_eq!(0, first_counter.0.load(Ordering::SeqCst));
        assert_eq!(1, latest_counter.0.load(Ordering::SeqCst));
        assert!(matches!(receiver.as_mut().poll(&mut latest_context), Poll::Ready("latest waiter")));
    }

    #[test]
    fn test_waker_clone_drop_and_wake_can_reenter_completion_state() {
        let (sender, receiver) = completion_pair::<usize>();
        let state = Arc::clone(&receiver.0);
        let (first_waker, first_probe) = lock_probe_waker(&state);
        let (latest_waker, latest_probe) = lock_probe_waker(&state);
        let mut receiver = Box::pin(receiver);

        let mut first_context = Context::from_waker(&first_waker);
        assert!(matches!(
            receiver.as_mut().poll(&mut first_context),
            Poll::Pending
        ));
        let mut latest_context = Context::from_waker(&latest_waker);
        assert!(matches!(
            receiver.as_mut().poll(&mut latest_context),
            Poll::Pending
        ));
        sender.complete(41);
        assert!(matches!(
            receiver.as_mut().poll(&mut latest_context),
            Poll::Ready(41)
        ));
        drop(receiver);
        drop(first_waker);
        drop(latest_waker);

        assert_eq!(0, first_probe.blocked.load(Ordering::SeqCst));
        assert_eq!(0, latest_probe.blocked.load(Ordering::SeqCst));
    }

    #[test]
    fn test_cancelled_receiver_unregisters_its_waker() {
        let (_sender, receiver) = completion_pair::<usize>();
        let state = Arc::clone(&receiver.0);
        let (waker, probe) = lock_probe_waker(&state);
        let mut context = Context::from_waker(&waker);
        let mut receiver = Box::pin(receiver);

        assert!(matches!(
            receiver.as_mut().poll(&mut context),
            Poll::Pending
        ));
        drop(receiver);
        assert!(state.lock().unwrap().waker.is_none());
        drop(waker);
        assert_eq!(0, probe.blocked.load(Ordering::SeqCst));
    }

    #[test]
    fn test_receiver_recovers_published_value_from_a_poisoned_state() {
        let (_sender, receiver) = completion_pair::<u8>();
        let state = Arc::clone(&receiver.0);
        let poisoner = std::thread::spawn(move || {
            let mut state = state.lock().unwrap();
            state.value = Some(19);
            panic!("poison completion state after publishing");
        });
        assert!(poisoner.join().is_err());

        let mut receiver = Box::pin(receiver);
        let waker = Waker::from(Arc::new(WakeCounter(AtomicUsize::new(0))));
        let mut context = Context::from_waker(&waker);
        assert!(matches!(receiver.as_mut().poll(&mut context), Poll::Ready(19)));
    }
}
