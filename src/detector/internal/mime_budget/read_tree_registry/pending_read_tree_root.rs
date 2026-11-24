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
    /// Pending index entry removed when the final binding ticket is dropped.
    pub(super) pending_roots: Weak<PendingRoots>,
    /// Parent operation whose weak index entry this root owns.
    pub(super) operation: OperationId,
}

/// Weak lookup table for pending roots keyed by their parent operation.
pub(super) type PendingRoots = Mutex<HashMap<OperationId, Weak<PendingReadTreeRoot>>>;

/// Removes a pending entry only while it still names the root being retired.
///
/// # Parameters
/// - `pending_roots`: Registry containing pending roots.
/// - `operation`: Parent operation associated with the root.
/// - `pending_root`: Root whose registration should be removed.
pub(super) fn unregister_pending(
    pending_roots: &PendingRoots,
    operation: OperationId,
    pending_root: &Arc<PendingReadTreeRoot>,
) {
    unregister_pending_ptr(pending_roots, operation, Arc::as_ptr(pending_root));
}

/// Removes a pending root only if the registry still holds the same allocation.
///
/// # Parameters
/// - `pending_roots`: Registry containing pending roots.
/// - `operation`: Parent operation associated with the root.
/// - `pending_root`: Allocation identity expected for the registered root.
fn unregister_pending_ptr(
    pending_roots: &PendingRoots,
    operation: OperationId,
    pending_root: *const PendingReadTreeRoot,
) {
    let mut pending_roots = pending_roots.lock().unwrap_or_else(|e| e.into_inner());
    let matches = pending_roots
        .get(&operation)
        .is_some_and(|registered| registered.as_ptr() == pending_root);
    if matches {
        pending_roots.remove(&operation);
    }
}

impl Drop for PendingReadTreeRoot {
    /// Removes this root's weak registration when its final owner is dropped.
    fn drop(&mut self) {
        if let Some(pending_roots) = self.pending_roots.upgrade() {
            unregister_pending_ptr(&pending_roots, self.operation, self as *const Self);
        }
    }
}
