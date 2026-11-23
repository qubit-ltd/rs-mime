// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::Arc;
use std::sync::Mutex;

use qubit_operation::OperationId;

use crate::detector::internal::mime_budget::ReadTreeContext;
use crate::detector::internal::mime_budget::read_tree_registry::pending_read_tree_root::PendingReadTreeRoot;
use crate::detector::internal::mime_budget::read_tree_registry::pending_read_tree_root::PendingRoots;
use crate::detector::internal::mime_budget::registration::Roots;

/// Describes whether a pending child binding borrows or creates the root.
pub(super) enum PendingBinding {
    /// Reuses a root that was already committed for the parent operation.
    Existing(
        /// Committed root whose budget is shared by the accepted child.
        Arc<ReadTreeContext>,
    ),
    /// Shares the first root reservation with sibling child submissions.
    First {
        /// Weak registrations retained while child contexts own the root.
        roots: Arc<Roots>,
        /// Pending roots shared by accepted siblings before the first commit.
        pending_roots: Arc<PendingRoots>,
        /// Lock that serializes pending and committed root registration.
        registrations: Arc<Mutex<()>>,
        /// Operation whose children share this root.
        operation: OperationId,
        /// Root and parent resource reservation for the first binding.
        pending_root: Arc<PendingReadTreeRoot>,
    },
}
