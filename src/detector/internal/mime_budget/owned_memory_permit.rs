// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use super::BudgetPermit;

/// Owned-memory reservation held while one bounded staging chunk is copied
/// and written by the execution worker.
#[must_use = "the owned-memory reservation must be held while staging data"]
pub(crate) struct OwnedMemoryPermit {
    /// Underlying owned-byte reservation retained for the staging chunk.
    pub(super) _permit: BudgetPermit,
}
