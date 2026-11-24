// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::Weak;

use qubit_operation::OperationId;

use super::index_entry::Roots;
use super::read_tree_context::ReadTreeContext;

/// Weak information needed to remove one context's operation index on drop.
#[derive(Debug)]
pub(super) struct Registration {
    /// Registry roots map, held weakly to avoid retaining its owner.
    pub(super) roots: Weak<Roots>,
    /// Operation ID associated with the weak context entry.
    pub(super) operation: OperationId,
    /// Context whose drop is responsible for removing this entry.
    pub(super) owner: Weak<ReadTreeContext>,
    /// Context returned when this operation identity is looked up.
    pub(super) target: Weak<ReadTreeContext>,
}
