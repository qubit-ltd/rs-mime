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

/// Readiness and its waiter share one lock, so a publisher cannot slip
/// between a poll's readiness check and waker registration.
/// Wakes registered pollers when a command lifecycle value is published.
///
/// # Type Parameters
/// - `T`: Lifecycle value published to waiting command observers.
pub(in crate::command_execution) struct PollSignal<T> {
    /// Value and waiter protected by one lock to prevent lost wakeups.
    state: Mutex<SignalState<T>>,
}

impl<T> PollSignal<T> {
    /// Creates an empty signal with no registered waiter.
    pub(in crate::command_execution) fn new() -> Self {
        Self {
            state: Mutex::new(SignalState {
                value: None,
                waker: None,
            }),
        }
    }

    /// Publishes one value and wakes the waiter registered by a pending poll.
    ///
    /// # Parameters
    /// - `value`: Lifecycle fact made available to pollers.
    pub(in crate::command_execution) fn publish(&self, value: T) {
        let waker = {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            state.value = Some(value);
            state.waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
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
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(value) = state.value.take() {
            state.waker = None;
            Poll::Ready(value)
        } else {
            state.waker = Some(cx.waker().clone());
            Poll::Pending
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
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(value) = state.value.as_ref() {
            Poll::Ready(value.clone())
        } else {
            state.waker = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}

#[cfg(test)]
mod tests {
    use std::task::Context;
    use std::task::Poll;
    use std::task::Waker;

    use super::PollSignal;

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
}
