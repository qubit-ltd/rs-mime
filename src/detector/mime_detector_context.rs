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
/// use qubit_mime::detector::MimeDetectorContext;
/// use qubit_mime::{MimeConfig, MimeRepository};
/// let context = MimeDetectorContext::new(
///     Arc::new(MimeConfig::default()),
///     Arc::new(MimeRepository::empty()),
/// );
/// assert!(context.config().max_buffer_size() > 0);
/// ```
#[derive(Debug, Clone)]
pub struct MimeDetectorContext {
    /// Detector configuration shared with this context's adapters.
    config: Arc<MimeConfig>,
    /// MIME repository shared with this context's adapters.
    repository: Arc<MimeRepository>,
}

impl MimeDetectorContext {
    /// Creates a context that shares the supplied configuration and repository.
    ///
    /// # Parameters
    /// - `config`: Shared detector configuration used by the context.
    /// - `repository`: Shared MIME repository used by detector adapters.
    ///
    /// # Returns
    /// A context that owns both shared handles without copying their contents.
    #[must_use]
    #[inline]
    pub fn new(config: Arc<MimeConfig>, repository: Arc<MimeRepository>) -> Self {
        Self { config, repository }
    }

    /// Returns the detector configuration shared by this context.
    ///
    /// # Returns
    /// A borrow of the configuration; the reference is valid while `self` is
    /// borrowed.
    #[must_use]
    #[inline]
    pub fn config(&self) -> &MimeConfig {
        &self.config
    }

    /// Returns the MIME repository shared by this context.
    ///
    /// # Returns
    /// A borrow of the repository; the reference is valid while `self` is
    /// borrowed.
    #[must_use]
    #[inline]
    pub fn repository(&self) -> &MimeRepository {
        &self.repository
    }

    /// Clones the shared repository handle for an adapter that owns its handle.
    ///
    /// Cloning the `Arc` increments its shared ownership count without copying
    /// the repository data.
    ///
    /// # Returns
    /// An owned handle to the same repository allocation.
    #[inline]
    pub(crate) fn repository_arc(&self) -> Arc<MimeRepository> {
        Arc::clone(&self.repository)
    }
}
