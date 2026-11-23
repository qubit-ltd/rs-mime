// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::Weak;

use qubit_operation::OperationId;

use crate::detector::internal::mime_budget::ReadTreeContext;
use crate::detector::internal::mime_budget::read_tree_registry::pending_read_tree_root_state::PendingReadTreeRootState;

/// Tracks one shared root while accepted sibling bindings await commit.
pub(super) struct PendingReadTreeRoot {
    /// Root budget shared by all children admitted through this reservation.
    pub(super) root: Arc<ReadTreeContext>,
    /// Parent resource reservation or its terminal commit state.
    pub(super) state: Mutex<PendingReadTreeRootState>,
}

/// Weak lookup table for pending roots keyed by their parent operation.
pub(super) type PendingRoots = Mutex<HashMap<OperationId, Weak<PendingReadTreeRoot>>>;
