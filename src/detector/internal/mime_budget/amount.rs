// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================

pub(super) enum Amount {
    Owned(usize),
    Staging(u64),
}
