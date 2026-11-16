// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Read totals tracked independently for one operation scope.
#[derive(Debug, Default)]
pub(super) struct ScopeCounters {
    /// Bytes committed by reads in this scope and its descendants.
    pub(super) consumed: u64,
    /// Bytes currently reserved by in-flight reads in this scope or
    /// descendants.
    pub(super) reserved: u64,
}
