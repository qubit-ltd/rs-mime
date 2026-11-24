// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Snapshotted async provider registration metadata.

use std::sync::Arc;

use super::super::AsyncMimeDetectorProvider;

/// Result of resolving a backend together with its trusted repository source.
pub(crate) type AsyncBackendSelection = (
    String,
    Arc<dyn crate::AsyncMimeDetectorBackend>,
    Option<Arc<crate::MimeRepository>>,
);

/// Async factory metadata retained for every selector registered to a provider.
#[derive(Clone)]
pub(crate) struct AsyncProviderEntry {
    /// Provider's canonical selector.
    pub(crate) id: String,
    /// Descriptor priority captured at registration.
    pub(crate) priority: i32,
    /// Whether this is one of the crate's built-in async factories.
    pub(crate) is_builtin: bool,
    /// Shared provider factory.
    pub(crate) provider: Arc<dyn AsyncMimeDetectorProvider>,
    /// Whether this exact crate built-in uses the bundled repository.
    pub(crate) uses_bundled_repository: bool,
}

impl std::fmt::Debug for AsyncProviderEntry {
    /// Formats the provider identity and trusted repository marker.
    ///
    /// # Parameters
    /// - `self`: Entry being formatted.
    /// - `f`: Formatter receiving the debug representation.
    ///
    /// # Returns
    /// The formatter after writing the debug representation.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AsyncProviderEntry")
            .field("id", &self.id)
            .field("priority", &self.priority)
            .field("is_builtin", &self.is_builtin)
            .field("uses_bundled_repository", &self.uses_bundled_repository)
            .finish_non_exhaustive()
    }
}
