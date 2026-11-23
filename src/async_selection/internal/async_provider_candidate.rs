// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Candidate snapshots retained while resolving asynchronous provider selections.

/// A canonical async provider factory or a registered provider with sync-only capability.
///
/// # Type Parameters
/// - `T`: Cloneable async provider factory snapshot associated with an async candidate.
#[derive(Clone)]
pub(crate) enum AsyncProviderCandidate<T> {
    /// A snapshotted factory capable of constructing the requested backend.
    Async {
        /// Canonical ID used for the selected provider.
        id: String,
        /// Cloned provider factory entry, safe to call after releasing catalog locks.
        provider: T,
    },
    /// A registered provider without an async factory; treated as absent capability.
    SyncOnly {
        /// Canonical ID of the sync-only provider.
        id: String,
    },
}
