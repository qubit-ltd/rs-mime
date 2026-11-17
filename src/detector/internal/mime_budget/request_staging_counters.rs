// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Counts current and peak bytes held by request staging reservations.
#[derive(Debug, Default)]
pub(super) struct RequestStagingCounters {
    /// Bytes currently held by live staging guards.
    pub(super) current: u64,
    /// Highest simultaneous byte count observed for this request.
    pub(super) peak: u64,
}
