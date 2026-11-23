// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Registration metadata retained for asynchronous classifier selectors.

use std::sync::Arc;

use super::super::AsyncMediaStreamClassifierProvider;

/// Factory and descriptor snapshot shared by a classifier ID and its aliases.
#[derive(Clone, Debug)]
pub(crate) struct AsyncProviderEntry {
    /// Canonical classifier ID returned by resolution.
    pub(crate) id: String,
    /// Descriptor priority used when the factory participates in Auto selection.
    pub(crate) priority: i32,
    /// Whether the factory is one of the crate's built-in providers.
    pub(crate) is_builtin: bool,
    /// Shared factory called after the async catalog lock is released.
    pub(crate) provider: Arc<dyn AsyncMediaStreamClassifierProvider>,
}
