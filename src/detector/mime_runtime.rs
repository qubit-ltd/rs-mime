// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::Arc;

pub use super::mime_detector_context::MimeDetectorContext;
use super::owned_repository_mime_detector::OwnedRepositoryMimeDetector;
use crate::MimeConfig;
use crate::MimeDetector;
use crate::MimeRepository;
use crate::MimeResult;

/// Process-local runtime owning shared MIME configuration and repository.
///
/// # Examples
///
/// ```
/// use qubit_mime::MimeDetector;
/// use qubit_mime::detector::MimeRuntime;
/// let runtime = MimeRuntime::builtin()?;
/// let detector = runtime.create_detector()?;
/// assert_eq!(detector.detect_by_filename("document.pdf")?, Some("application/pdf".to_owned()));
/// # Ok::<(), qubit_mime::MimeError>(())
/// ```
#[derive(Debug, Clone)]
pub struct MimeRuntime {
    context: MimeDetectorContext,
}

impl MimeRuntime {
    /// Creates a runtime that shares the supplied repository handle.
    ///
    /// The configuration is moved into a shared handle so detectors created by
    /// this runtime use the same immutable settings.
    ///
    /// # Parameters
    ///
    /// - `config`: detection settings used by detectors created by this
    ///   runtime.
    /// - `repository`: shared MIME repository used for detection.
    ///
    /// # Returns
    ///
    /// A runtime retaining the configuration and repository for later detector
    /// creation.
    pub fn new(config: MimeConfig, repository: Arc<MimeRepository>) -> Self {
        Self {
            context: MimeDetectorContext::new(Arc::new(config), repository),
        }
    }

    /// Creates a runtime using the default configuration and bundled
    /// repository.
    ///
    /// # Returns
    ///
    /// A runtime backed by the crate's bundled MIME data.
    pub fn builtin() -> MimeResult<Self> {
        Ok(Self::new(MimeConfig::default(), MimeRepository::bundled_shared()))
    }

    /// Returns the immutable configuration and repository shared by this
    /// runtime.
    ///
    /// # Returns
    ///
    /// A reference to this runtime's context. The reference is valid for the
    /// lifetime of the borrow and does not clone the shared handles.
    pub fn context(&self) -> &MimeDetectorContext {
        &self.context
    }

    /// Creates a detector using this runtime's configuration and repository.
    ///
    /// The detector receives a clone of the configuration value and a clone of
    /// the repository's `Arc`, so the repository remains alive independently of
    /// this runtime.
    ///
    /// # Returns
    ///
    /// A shared handle to a repository-backed MIME detector.
    pub fn create_detector(&self) -> MimeResult<Arc<dyn MimeDetector>> {
        Ok(Arc::new(OwnedRepositoryMimeDetector::new(
            self.context.config().clone(),
            self.context.repository_arc(),
        )))
    }
}

impl Default for MimeRuntime {
    /// Creates a runtime containing the built-in providers and repository.
    fn default() -> Self {
        Self::builtin().expect("built-in MIME runtime should initialize")
    }
}
