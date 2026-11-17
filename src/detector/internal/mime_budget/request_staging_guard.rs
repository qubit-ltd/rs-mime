// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::Arc;

use super::request_staging_usage::RequestStagingUsage;

/// Releases current request usage when the corresponding staging permit ends.
#[must_use = "the staging reservation must be held until it is released"]
pub(crate) struct RequestStagingGuard {
    /// Shared request counters decremented when this guard is dropped.
    pub(super) usage: Arc<RequestStagingUsage>,
    /// Number of bytes held by this guard.
    pub(super) bytes: u64,
}

impl Drop for RequestStagingGuard {
    /// Releases this guard's bytes from the request counters.
    ///
    /// # Panics
    /// Panics if the current counter is smaller than this guard's reservation.
    fn drop(&mut self) {
        let mut counters = self.usage.counters.lock().unwrap_or_else(|error| error.into_inner());
        counters.current = counters
            .current
            .checked_sub(self.bytes)
            .expect("staging usage guard underflow");
    }
}
