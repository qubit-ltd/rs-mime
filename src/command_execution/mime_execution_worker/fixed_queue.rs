// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Bounded queue used by the fixed MIME worker pool.

use std::sync::Condvar;
use std::sync::Mutex;

use super::queue_state::QueueState;

/// Queue storage, wake-up signal, and maximum pending-job count.
pub(super) struct FixedQueue {
    /// Mutable queue state.
    pub(super) state: Mutex<QueueState>,
    /// Notifies workers when jobs arrive or the queue closes.
    pub(super) ready: Condvar,
    /// Maximum number of pending jobs.
    pub(super) capacity: usize,
}
