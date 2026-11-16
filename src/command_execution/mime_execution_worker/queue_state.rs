// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! State shared by the fixed worker queue.

use std::collections::VecDeque;

/// Pending jobs and the queue's shutdown state.
pub(super) struct QueueState {
    /// Jobs waiting for a worker.
    pub(super) jobs: VecDeque<Box<dyn FnOnce() + Send + 'static>>,
    /// Whether workers should exit once pending jobs are drained.
    pub(super) closed: bool,
}
