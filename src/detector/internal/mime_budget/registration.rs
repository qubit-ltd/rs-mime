// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::Weak;

use qubit_operation::OperationId;

use super::ReadTreeContext;

/// Weak registry entries keyed by operation identity.
pub(super) type Roots = Mutex<HashMap<OperationId, Weak<ReadTreeContext>>>;

/// Weak information needed to remove one context's operation index on drop.
#[derive(Debug)]
pub(super) struct Registration {
    /// Registry roots map, held weakly to avoid retaining its owner.
    pub(super) roots: Weak<Roots>,
    /// Operation ID associated with the weak context entry.
    pub(super) operation: OperationId,
}
