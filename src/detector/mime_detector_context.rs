// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Shared configuration and repository context for MIME detectors.

use std::sync::Arc;

use crate::MimeConfig;
use crate::MimeRepository;

/// Immutable context shared by detector adapters.
///
/// # Examples
///
/// ```
/// use std::sync::Arc;
/// use qubit_mime::{MimeConfig, MimeRepository};
/// use qubit_mime::detector::MimeDetectorContext;
/// let context = MimeDetectorContext::new(Arc::new(MimeConfig::default()), Arc::new(MimeRepository::empty()));
/// assert!(context.config().max_buffer_size() > 0);
/// ```
#[derive(Debug, Clone)]
pub struct MimeDetectorContext {
    config: Arc<MimeConfig>,
    repository: Arc<MimeRepository>,
}

impl MimeDetectorContext {
    /// Creates a context from configuration and repository.
    pub fn new(config: Arc<MimeConfig>, repository: Arc<MimeRepository>) -> Self {
        Self { config, repository }
    }

    /// Returns detector configuration.
    pub fn config(&self) -> &MimeConfig {
        &self.config
    }

    /// Returns MIME repository.
    pub fn repository(&self) -> &MimeRepository {
        &self.repository
    }

    /// Clones the shared repository handle.
    pub(crate) fn repository_arc(&self) -> Arc<MimeRepository> {
        Arc::clone(&self.repository)
    }
}
