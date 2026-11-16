// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Shared value and waiter state for a completion channel.

/// Shared state observed by the sender and receiver halves.
///
/// # Type Parameters
/// - `T`: Value transferred from the sender to the receiver.
pub(in crate::managed_temp_staging) struct Completion<T> {
    /// Result waiting to be received.
    pub(in crate::managed_temp_staging) value: Option<T>,
    /// Task to wake when a result is published.
    pub(in crate::managed_temp_staging) waker: Option<std::task::Waker>,
}
