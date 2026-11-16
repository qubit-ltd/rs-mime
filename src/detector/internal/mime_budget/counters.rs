// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::collections::HashMap;
use std::sync::Weak;

use super::PermitInner;

/// Current owned-memory and staging reservations plus allocation identities.
#[derive(Debug, Default)]
pub(super) struct Counters {
    /// Total bytes of distinct owned allocations currently reserved.
    pub(super) owned: usize,
    /// Total staging bytes currently reserved.
    pub(super) staging: u64,
    /// Weak allocation and permit references used to avoid double-counting an
    /// `Arc`.
    pub(super) allocations: HashMap<usize, (Weak<[u8]>, Weak<PermitInner>)>,
}
