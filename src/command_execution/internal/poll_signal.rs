// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Lock-protected one-shot and snapshot signals used by command lifecycle
//! state.

use std::sync::Mutex;
use std::task::Context;
use std::task::Poll;

use super::SignalState;
use crate::safe_wake::wake_safely;

/// Readiness and its waiter share one lock, so a publisher cannot slip
/// between a poll's readiness check and waker registration.
/// Wakes registered pollers when a command lifecycle value is published.
///
/// # Type Parameters
/// - `T`: Lifecycle value published to waiting command observers.
#[must_use]
pub(in crate::command_execution) struct PollSignal<T> {
    /// Value and waiter protected by one lock to prevent lost wakeups.
    state: Mutex<SignalState<T>>,
}

impl<T> PollSignal<T> {
    /// Creates an empty signal with no registered waiter.
    ///
    /// # Returns
    /// An empty signal ready to publish a value or register a waiter.
    pub(in crate::command_execution) fn new() -> Self {
        Self {
            state: Mutex::new(SignalState {
                value: None,
                waker: None,
            }),
        }
    }

    /// Clones the value when already published without registering a waiter.
    ///
    /// # Returns
    /// A clone of the value, or `None` while publication is pending.
    pub(in crate::command_execution) fn value(&self) -> Option<T>
    where
        T: Clone,
    {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).value.clone()
    }

    /// Publishes one value and wakes the waiter registered by a pending poll.
    ///
    /// # Parameters
    /// - `value`: Lifecycle fact made available to pollers.
    pub(in crate::command_execution) fn publish(&self, value: T) {
        let (old_value, waker) = {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            (state.value.replace(value), state.waker.take())
        };
        drop(old_value);
        if let Some(waker) = waker {
            wake_safely(waker);
        }
    }

    /// Takes the published value or registers the current task for a wakeup.
    ///
    /// # Parameters
    /// - `cx`: Poll context used to register the waiting task.
    ///
    /// # Returns
    /// `Ready` with the single value, or `Pending` after registering the waker.
    pub(in crate::command_execution) fn poll_take(&self, cx: &mut Context<'_>) -> Poll<T> {
        let mut candidate = Some(cx.waker().clone());
        let (result, old_waker) = {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(value) = state.value.take() {
                (Poll::Ready(value), state.waker.take())
            } else {
                (
                    Poll::Pending,
                    state
                        .waker
                        .replace(candidate.take().expect("candidate waker is present")),
                )
            }
        };
        drop(old_waker);
        if matches!(result, Poll::Ready(_)) {
            drop(candidate);
        }
        result
    }

    /// Clones a published value or registers the current task for a wakeup.
    ///
    /// # Parameters
    /// - `cx`: Poll context used to register the waiting task.
    ///
    /// # Returns
    /// `Ready` with a clone, or `Pending` after registering the waker.
    pub(in crate::command_execution) fn poll_clone(&self, cx: &mut Context<'_>) -> Poll<T>
    where
        T: Clone,
    {
        let mut candidate = Some(cx.waker().clone());
        let (result, old_waker) = {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(value) = state.value.as_ref() {
                (Poll::Ready(value.clone()), None)
            } else {
                (
                    Poll::Pending,
                    state
                        .waker
                        .replace(candidate.take().expect("candidate waker is present")),
                )
            }
        };
        drop(old_waker);
        if matches!(result, Poll::Ready(_)) {
            drop(candidate);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::Weak;
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::Ordering;
    use std::task::Context;
    use std::task::Poll;
    use std::task::Wake;
    use std::task::Waker;

    use super::PollSignal;

    struct ReenterSignalWake {
        signal: Weak<PollSignal<u8>>,
        lock_was_free: Arc<AtomicBool>,
    }

    impl Wake for ReenterSignalWake {
        fn wake(self: Arc<Self>) {
            let signal = self.signal.upgrade().expect("signal remains alive");
            self.lock_was_free
                .store(signal.state.try_lock().is_ok(), Ordering::SeqCst);
        }
    }

    #[test]
    fn test_cloned_observation_does_not_consume_the_published_value() {
        let signal = PollSignal::new();
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);

        assert_eq!(None, signal.value());
        assert!(matches!(signal.poll_clone(&mut context), Poll::Pending));
        signal.publish(String::from("completed"));
        assert_eq!(Some(String::from("completed")), signal.value());
        assert!(matches!(signal.poll_clone(&mut context), Poll::Ready(value) if value == "completed"));
        assert_eq!(Some(String::from("completed")), signal.value());
        assert!(matches!(signal.poll_take(&mut context), Poll::Ready(value) if value == "completed"));
        assert_eq!(None, signal.value());
    }

    #[test]
    fn test_poisoned_signal_state_recovers_for_publish_and_poll() {
        let signal = PollSignal::new();
        let poison = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut state = signal.state.lock().unwrap();
            state.value = Some(3_u8);
            panic!("poison poll signal state");
        }));
        assert!(poison.is_err());

        assert_eq!(Some(3), signal.value());
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);
        assert!(matches!(signal.poll_clone(&mut context), Poll::Ready(3)));
        signal.publish(7);
        assert_eq!(Some(7), signal.value());
        assert!(matches!(signal.poll_take(&mut context), Poll::Ready(7)));
        assert_eq!(None, signal.value());
    }

    #[test]
    fn test_wake_can_reenter_signal_after_publication() {
        let signal = Arc::new(PollSignal::new());
        let lock_was_free = Arc::new(AtomicBool::new(false));
        let waker = Waker::from(Arc::new(ReenterSignalWake {
            signal: Arc::downgrade(&signal),
            lock_was_free: Arc::clone(&lock_was_free),
        }));
        let mut context = Context::from_waker(&waker);
        assert!(matches!(signal.poll_clone(&mut context), Poll::Pending));

        signal.publish(9);

        assert!(lock_was_free.load(Ordering::SeqCst));
        assert_eq!(Some(9), signal.value());
    }

    struct PanicWake;
    impl Wake for PanicWake {
        fn wake(self: Arc<Self>) {
            panic!("observer wake failed");
        }
    }

    #[test]
    fn test_panicking_wake_does_not_interrupt_published_value() {
        let signal = PollSignal::new();
        let waker = Waker::from(Arc::new(PanicWake));
        let mut context = Context::from_waker(&waker);
        assert!(matches!(signal.poll_clone(&mut context), Poll::Pending));

        signal.publish(11);

        assert_eq!(Some(11), signal.value());
    }
}
