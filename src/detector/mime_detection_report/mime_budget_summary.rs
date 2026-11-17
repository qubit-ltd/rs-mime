// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
/// Summary of bytes retained, read, and temporarily staged for one request.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MimeBudgetSummary {
    /// Bytes charged to owned-memory budget at the report boundary.
    pub owned_bytes: usize,
    /// Bytes actually read from providers, including repeated reads.
    pub bytes_read: u64,
    /// Peak staging bytes simultaneously charged to this request.
    ///
    /// This is a high-water mark, not cumulative bytes written or the bytes
    /// still retained when the report is assembled. Concurrent requests are
    /// accounted independently. It also does not indicate that resource
    /// cleanup or operation settlement has completed.
    pub staging_bytes: u64,
}
