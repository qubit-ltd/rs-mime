// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================

#[derive(Debug, Clone, Copy)]
pub(in crate::detector) struct Limits {
    pub owned: usize,
    pub staging: u64,
}
