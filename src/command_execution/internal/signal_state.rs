// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Mutable state shared by command completion signal operations.

use std::task::Waker;

/// State shared by a signal publisher and its registered waiter.
///
/// # Type Parameters
/// - `T`: Value published to signal observers.
pub(in crate::command_execution) struct SignalState<T> {
    /// Published value, retained until taken or cloned by a poller.
    pub(super) value: Option<T>,
    /// Most recent task waiting for publication.
    pub(super) waker: Option<Waker>,
}
