// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Kind and size of a reservation tracked by the MIME memory budget.
pub(super) enum Amount {
    /// Owned content allocation memory.
    Owned(
        /// Bytes owned by in-memory MIME content.
        usize,
    ),
    /// Capacity reserved for a staging resource.
    Staging(
        /// Bytes reserved for staged resources.
        u64,
    ),
}
