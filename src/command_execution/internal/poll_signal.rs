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
