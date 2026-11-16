// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use super::BudgetPermit;

/// RAII reservation held for as long as the staged resource remains owned.
pub(crate) struct StagingPermit {
    /// Underlying staging-byte reservation retained with the resource.
    pub(super) _permit: BudgetPermit,
}
