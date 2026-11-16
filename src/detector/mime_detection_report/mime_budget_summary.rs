// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
/// Summary of bytes retained and read while serving a request.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MimeBudgetSummary {
    /// Bytes charged to owned-memory budget at the report boundary.
    pub owned_bytes: usize,
    /// Bytes actually read from providers, including repeated reads.
    pub bytes_read: u64,
    /// Peak temporary storage charged to the request.
    pub staging_bytes: u64,
}
