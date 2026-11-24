// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Stop-claim cleanup for the MIME shutdown observer.

use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

/// Releases the single stop execution claim on every exit, including panic.
pub(super) struct StopPermit<'a> {
    /// Atomic flag that allows only one stop callback at a time.
    pub(super) stopping: &'a AtomicBool,
}

/// Releases the stop claim after the shutdown attempt ends.
impl Drop for StopPermit<'_> {
    /// Makes a later shutdown attempt eligible to retry.
    fn drop(&mut self) {
        self.stopping.store(false, Ordering::Release);
    }
}
