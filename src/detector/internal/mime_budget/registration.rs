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

use super::ReadTreeContext;

/// A weak index entry separates the context that owns cleanup from the
/// context returned by lookup.
#[derive(Debug)]
pub(super) struct IndexEntry {
    /// Context whose drop is responsible for removing this entry.
    pub(super) owner: Weak<ReadTreeContext>,
    /// Context returned when this operation identity is looked up.
    pub(super) target: Weak<ReadTreeContext>,
}

impl IndexEntry {
    /// Creates a weak index entry with explicit cleanup and lookup identities.
    pub(super) fn new(owner: &Arc<ReadTreeContext>, target: &Arc<ReadTreeContext>) -> Self {
        Self {
            owner: Arc::downgrade(owner),
            target: Arc::downgrade(target),
        }
    }

    /// Upgrades the indexed lookup target while it remains alive.
    pub(super) fn upgrade_target(&self) -> Option<Arc<ReadTreeContext>> {
        self.target.upgrade()
    }
}

/// Weak registry entries keyed by operation identity.
pub(super) type Roots = Mutex<HashMap<OperationId, IndexEntry>>;

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
