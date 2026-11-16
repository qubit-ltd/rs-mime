// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================

use std::sync::Arc;

use super::PermitInner;

/// Cloneable reservation whose count returns only after its final owner drops.
pub(in crate::detector) struct BudgetPermit {
    pub(super) _inner: Arc<PermitInner>,
}
