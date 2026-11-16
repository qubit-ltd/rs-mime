// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Started worker threads and their shared queue.

use std::sync::Arc;
use std::thread::JoinHandle;

use super::fixed_queue::FixedQueue;

/// Resources for a prepared worker pool.
pub(super) struct RunningWorkers {
    /// Queue shared by all worker threads.
    pub(super) queue: Arc<FixedQueue>,
    /// Started worker threads.
    pub(super) threads: Vec<JoinHandle<()>>,
}
