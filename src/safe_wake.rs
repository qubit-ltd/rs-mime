// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Panic-isolated notification for task wakers supplied by callers.

use std::panic;
use std::panic::AssertUnwindSafe;
use std::task::Waker;

/// Wakes a task while preventing a misbehaving observer from blocking others.
///
/// # Parameters
/// - `waker`: Caller-supplied task waker to invoke.
pub(crate) fn wake_safely(waker: Waker) {
    if let Err(payload) = panic::catch_unwind(AssertUnwindSafe(|| waker.wake())) {
        dispose_panic_payload(payload);
    }
}

/// Drops a panic payload while containing a panic from its destructor.
///
/// # Parameters
/// - `payload`: Panic payload to dispose of inside an unwind boundary.
fn dispose_panic_payload(payload: Box<dyn std::any::Any + Send + 'static>) {
    if let Err(secondary) = panic::catch_unwind(AssertUnwindSafe(|| drop(payload))) {
        // A panicking payload cannot be safely freed after its destructor fails.
        std::mem::forget(secondary);
    }
}
