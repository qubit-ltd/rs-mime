// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Mutable count and wake queue shared by in-flight admission guards.

use std::task::Waker;

/// State protected by the admission tracker mutex.
///
/// The count and waiter queue are updated together so a waiter cannot miss the
/// transition to zero while registering its waker.
#[derive(Default)]
pub(in crate::detector::mime_execution_services) struct AdmissionState {
    /// Number of admission callbacks that still hold their in-flight guards.
    pub(in crate::detector::mime_execution_services) active: usize,
    /// Tasks waiting for the active admission count to reach zero.
    pub(in crate::detector::mime_execution_services) waiters: Vec<Waker>,
}
