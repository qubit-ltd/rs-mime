// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Registration metadata retained for asynchronous classifier selectors.

use std::fmt;
use std::sync::Arc;

use super::super::AsyncMediaStreamClassifierProvider;

/// Factory and descriptor snapshot shared by a classifier ID and its aliases.
#[derive(Clone)]
pub(crate) struct AsyncProviderEntry {
    /// Canonical classifier ID returned by resolution.
    pub(crate) id: String,
    /// Descriptor priority used when the factory participates in Auto
    /// selection.
    pub(crate) priority: i32,
    /// Whether the factory is one of the crate's built-in providers.
    pub(crate) is_builtin: bool,
    /// Shared factory called after the async catalog lock is released.
    pub(crate) provider: Arc<dyn AsyncMediaStreamClassifierProvider>,
}

impl fmt::Debug for AsyncProviderEntry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AsyncProviderEntry")
            .field("id", &self.id)
            .field("priority", &self.priority)
            .field("is_builtin", &self.is_builtin)
            .finish_non_exhaustive()
    }
}
