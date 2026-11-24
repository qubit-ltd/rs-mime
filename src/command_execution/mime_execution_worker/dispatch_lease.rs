// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Drop guard that completes an accepted job discarded before execution.

use std::sync::Arc;
use std::sync::Mutex;

use super::GuardState;
use super::take_abandoned;

/// Holds the state lease whose drop detects destruction of an accepted job.
///
/// # Type Parameters
/// - `T`: Payload transferred to the worker.
/// - `Run`: Callback executed when the worker starts.
/// - `Abandon`: Callback used when an accepted job is discarded first.
pub(super) struct DispatchLease<T, Run, Abandon: FnOnce(T)> {
    /// Shared lifecycle state observed when this lease is dropped.
    pub(super) state: Arc<Mutex<GuardState<T, Run, Abandon>>>,
}

impl<T, Run, Abandon> Drop for DispatchLease<T, Run, Abandon>
where
    Abandon: FnOnce(T),
{
    /// Reports a terminal result when an accepted queued job is destroyed.
    fn drop(&mut self) {
        let abandoned = {
            let mut state = self.state.lock().unwrap_or_else(|poison| poison.into_inner());
            state.dropped = true;
            take_abandoned(&mut state)
        };
        if let Some((payload, abandon)) = abandoned {
            abandon(payload);
        }
    }
}
