// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Sender half of a completion channel.

use std::sync::Arc;
use std::sync::Mutex;

use super::Completion;

/// Publishes one result to a waiting completion future.
///
/// # Type Parameters
/// - `T`: Result value shared with the receiver.
pub(in crate::managed_temp_staging) struct Completer<T>(
    #[doc = "Shared result and receiver-waker state."] pub(in crate::managed_temp_staging) Arc<Mutex<Completion<T>>>,
);

impl<T> Completer<T> {
    /// Stores the result and wakes a receiver that is already waiting.
    ///
    /// # Parameters
    /// - `value`: Result to publish to the receiver.
    ///
    /// # Returns
    /// Nothing; the waiting receiver is woken when needed.
    pub(in crate::managed_temp_staging) fn complete(self, value: T) {
        let mut state = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        state.value = Some(value);
        if let Some(waker) = state.waker.take() {
            waker.wake();
        }
    }
}
