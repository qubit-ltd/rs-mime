// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================

#[derive(Debug, Default)]
pub(super) struct ReadCounters {
    pub(super) consumed: u64,
    pub(super) reserved: u64,
}
