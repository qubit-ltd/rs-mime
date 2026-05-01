/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Boxed MIME detector wrapper.

use std::ops::Deref;

use crate::{MimeConfig, MimeDetectionPolicy, MimeDetector};

use super::mime_detector_backend::MimeDetectorBackend;
use super::{FileCommandMimeDetector, RepositoryMimeDetector};

/// A MIME detector stored in a [`Box`].
pub struct BoxMimeDetector {
    inner: Box<dyn MimeDetector>,
}

impl BoxMimeDetector {
    /// Wraps an existing boxed MIME detector.
    ///
    /// # Parameters
    /// - `detector`: Detector to wrap.
    ///
    /// # Returns
    /// Wrapped boxed detector.
    pub fn new(detector: Box<dyn MimeDetector>) -> Self {
        Self { inner: detector }
    }

    /// Creates a boxed detector from an implementation name.
    ///
    /// # Parameters
    /// - `name`: Detector selector.
    ///
    /// # Returns
    /// Matching detector, or `None` when the selector is empty or unknown.
    pub fn from_name(name: &str) -> Option<Self> {
        MimeDetectorBackend::from_name(name).map(Self::from_backend)
    }

    /// Creates a boxed detector from MIME configuration.
    ///
    /// # Parameters
    /// - `config`: MIME configuration containing the default detector selector.
    ///
    /// # Returns
    /// Configured detector wrapper.
    pub fn from_mime_config(config: &MimeConfig) -> Self {
        let backend = MimeDetectorBackend::select(
            config.mime_detector_default(),
            FileCommandMimeDetector::is_available(),
        );
        Self::from_backend_with_config(backend, config)
    }

    /// Unwraps this wrapper into the inner boxed detector.
    ///
    /// # Returns
    /// Inner boxed detector.
    pub fn into_inner(self) -> Box<dyn MimeDetector> {
        self.inner
    }

    fn from_backend(backend: MimeDetectorBackend) -> Self {
        Self::from_backend_with_config(backend, &MimeConfig::default())
    }

    fn from_backend_with_config(backend: MimeDetectorBackend, config: &MimeConfig) -> Self {
        match backend {
            MimeDetectorBackend::Repository => Self::new(Box::new(
                RepositoryMimeDetector::from_mime_config(config.clone()),
            )),
            MimeDetectorBackend::FileCommand => Self::new(Box::new(
                FileCommandMimeDetector::from_mime_config(config.clone()),
            )),
        }
    }
}

impl Default for BoxMimeDetector {
    fn default() -> Self {
        Self::from_mime_config(&MimeConfig::default())
    }
}

impl Deref for BoxMimeDetector {
    type Target = dyn MimeDetector;

    fn deref(&self) -> &Self::Target {
        self.as_ref()
    }
}

impl AsRef<dyn MimeDetector> for BoxMimeDetector {
    fn as_ref(&self) -> &(dyn MimeDetector + 'static) {
        self.inner.as_ref()
    }
}

impl From<Box<dyn MimeDetector>> for BoxMimeDetector {
    fn from(detector: Box<dyn MimeDetector>) -> Self {
        Self::new(detector)
    }
}

impl From<BoxMimeDetector> for Box<dyn MimeDetector> {
    fn from(detector: BoxMimeDetector) -> Self {
        detector.into_inner()
    }
}

impl MimeDetector for BoxMimeDetector {
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
