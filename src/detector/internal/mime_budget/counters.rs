// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================

use std::collections::HashMap;
use std::sync::Weak;

use super::PermitInner;

#[derive(Debug, Default)]
pub(super) struct Counters {
    pub(super) owned: usize,
    pub(super) staging: u64,
    pub(super) allocations: HashMap<usize, (Weak<[u8]>, Weak<PermitInner>)>,
}
