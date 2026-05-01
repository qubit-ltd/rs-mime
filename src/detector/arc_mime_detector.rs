/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Shared MIME detector wrapper.

use std::ops::Deref;
use std::sync::Arc;

use crate::{ENV_MIME_DETECTOR_DEFAULT, MimeDetectionPolicy, MimeDetector};

use super::mime_detector_backend::MimeDetectorBackend;
use super::{FileCommandMimeDetector, RepositoryMimeDetector};

/// A MIME detector stored in an [`Arc`].
#[derive(Clone)]
pub struct ArcMimeDetector {
    inner: Arc<dyn MimeDetector>,
}

impl ArcMimeDetector {
    /// Wraps an existing shared MIME detector.
    ///
    /// # Parameters
    /// - `detector`: Detector to wrap.
    ///
    /// # Returns
    /// Wrapped shared detector.
    pub fn new(detector: Arc<dyn MimeDetector>) -> Self {
        Self { inner: detector }
    }

    /// Creates a shared detector from an implementation name.
    ///
    /// # Parameters
    /// - `name`: Detector selector.
    ///
    /// # Returns
    /// Matching detector, or `None` when the selector is empty or unknown.
    pub fn from_name(name: &str) -> Option<Self> {
        super::mime_detector_backend::MimeDetectorBackend::from_name(name).map(Self::from_backend)
    }

    /// Unwraps this wrapper into the inner shared detector.
    ///
    /// # Returns
    /// Inner shared detector.
    pub fn into_inner(self) -> Arc<dyn MimeDetector> {
        self.inner
    }

    fn from_backend(backend: MimeDetectorBackend) -> Self {
        match backend {
            MimeDetectorBackend::Repository => {
                Self::new(Arc::new(RepositoryMimeDetector::default()))
            }
            MimeDetectorBackend::FileCommand => Self::new(Arc::new(FileCommandMimeDetector::new())),
        }
    }
}

impl Default for ArcMimeDetector {
    fn default() -> Self {
        let configured = std::env::var(ENV_MIME_DETECTOR_DEFAULT).unwrap_or_default();
        let backend = super::mime_detector_backend::MimeDetectorBackend::select(
            &configured,
            FileCommandMimeDetector::is_available(),
        );
        Self::from_backend(backend)
    }
}

impl Deref for ArcMimeDetector {
    type Target = dyn MimeDetector;

    fn deref(&self) -> &Self::Target {
        self.as_ref()
    }
}

impl AsRef<dyn MimeDetector> for ArcMimeDetector {
    fn as_ref(&self) -> &(dyn MimeDetector + 'static) {
        self.inner.as_ref()
    }
}

impl From<Arc<dyn MimeDetector>> for ArcMimeDetector {
    fn from(detector: Arc<dyn MimeDetector>) -> Self {
        Self::new(detector)
    }
}

impl From<ArcMimeDetector> for Arc<dyn MimeDetector> {
    fn from(detector: ArcMimeDetector) -> Self {
        detector.into_inner()
    }
}

impl MimeDetector for ArcMimeDetector {
    fn detect_by_filename(&self, filename: &str) -> Option<String> {
        self.inner.detect_by_filename(filename)
    }

    fn detect_by_content(&self, content: &[u8]) -> Option<String> {
        self.inner.detect_by_content(content)
    }

    fn detect(
        &self,
        content: &[u8],
        filename: Option<&str>,
        policy: MimeDetectionPolicy,
    ) -> Option<String> {
        self.inner.detect(content, filename, policy)
    }
}
