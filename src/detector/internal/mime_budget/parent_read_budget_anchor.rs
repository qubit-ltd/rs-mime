// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::Arc;

use super::ReadTreeContext;

/// Holds an external parent's cumulative read tree until scope cleanup.
#[derive(Debug)]
pub(super) struct ParentReadBudgetAnchor {
    /// Read context retained by the resource scope.
    _context: Arc<ReadTreeContext>,
}

impl ParentReadBudgetAnchor {
    /// Creates an anchor for one external-parent read tree.
    ///
    /// # Parameters
    /// - `context`: The read tree to retain for the parent scope lifetime.
    pub(super) fn new(context: Arc<ReadTreeContext>) -> Self {
        Self { _context: context }
    }
}
