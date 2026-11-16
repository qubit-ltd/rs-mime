// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Creates a sender and receiver pair for staging worker completion.

use std::sync::Arc;
use std::sync::Mutex;

use super::Completer;
use super::Completion;
use super::CompletionFuture;

/// Creates a one-shot channel for returning a worker result.
///
/// # Type Parameters
/// - `T`: Result value transferred through the channel.
///
/// # Returns
/// A sender and future receiver sharing one completion state.
pub(in crate::managed_temp_staging) fn completion_pair<T>() -> (Completer<T>, CompletionFuture<T>) {
    let state = Arc::new(Mutex::new(Completion {
        value: None,
        waker: None,
    }));
    (Completer(Arc::clone(&state)), CompletionFuture(state))
}
