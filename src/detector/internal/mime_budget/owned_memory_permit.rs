// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================

use super::BudgetPermit;

/// Owned-memory reservation held while one bounded staging chunk is copied
/// and written by the execution worker.
pub(crate) struct OwnedMemoryPermit {
    pub(super) _permit: BudgetPermit,
}
