// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Maximum memory and staging usage allowed for MIME execution services.
#[derive(Debug, Clone, Copy)]
pub(in crate::detector) struct Limits {
    /// Maximum number of bytes in owned MIME allocations.
    pub owned: usize,
    /// Maximum number of bytes held in staging resources.
    pub staging: u64,
}
