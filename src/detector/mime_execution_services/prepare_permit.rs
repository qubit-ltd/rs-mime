// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Prepare-claim cleanup for MIME execution services.

use std::sync::atomic::Ordering;

use super::MimeExecutionServices;

/// Releases the prepare claim on every return path, including unwinding.
pub(super) struct PreparePermit<'a> {
    /// Service whose prepare callback owns the claim.
    pub(super) services: &'a MimeExecutionServices,
    /// Whether preparation reached its completion path.
    pub(super) completed: bool,
}

/// Clears or closes lifecycle state when preparation exits.
impl Drop for PreparePermit<'_> {
    /// Releases the prepare claim and closes a draining failed startup.
    fn drop(&mut self) {
        if !self.completed {
            let _lifecycle = self.services.lifecycle_lock.lock().unwrap_or_else(|e| e.into_inner());
            if self.services.lifecycle() == super::MimeServiceLifecycle::Draining {
                self.services
                    .lifecycle
                    .store(super::MimeServiceLifecycle::ClosedWithoutStart as u8, Ordering::Release);
            }
            self.services.preparing.store(false, Ordering::Release);
        } else {
            self.services.preparing.store(false, Ordering::Release);
        }
    }
}
