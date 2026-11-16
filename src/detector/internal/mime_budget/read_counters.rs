// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Read bytes committed and provisionally reserved for one budget scope.
#[derive(Debug, Default)]
pub(super) struct ReadCounters {
    /// Bytes successfully read and charged to the scope.
    pub(super) consumed: u64,
    /// Capacity reserved for reads that have not yet completed.
    pub(super) reserved: u64,
}
