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
        let mut state = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        if let Some(value) = state.value.take() {
            Poll::Ready(value)
        } else {
            state.waker = Some(context.waker().clone());
            Poll::Pending
        }
    }
}
