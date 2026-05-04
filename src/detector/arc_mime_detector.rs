/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Shared MIME detector wrapper.

use std::ops::Deref;
use std::path::Path;
use std::sync::Arc;

use qubit_io::ReadSeek;

use crate::{
    MimeConfig,
    MimeDetectionPolicy,
    MimeDetector,
    MimeResult,
};

use super::mime_detector_kind::MimeDetectorKind;
use super::{
    FileCommandMimeDetector,
    RepositoryMimeDetector,
};

/// A MIME detector stored in an [`Arc`].
#[derive(Debug, Clone)]
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
        MimeDetectorKind::from_name(name).map(Self::from_kind)
    }

    /// Creates a shared detector from MIME configuration.
    ///
    /// # Parameters
    /// - `config`: MIME configuration containing the default detector selector.
    ///
    /// # Returns
    /// Configured detector wrapper.
    pub fn from_config(config: &MimeConfig) -> Self {
        let kind = MimeDetectorKind::select(
            config.mime_detector_default(),
            FileCommandMimeDetector::is_available(),
        );
        Self::from_kind_with_config(kind, config)
    }

    /// Unwraps this wrapper into the inner shared detector.
    ///
    /// # Returns
    /// Inner shared detector.
    pub fn into_inner(self) -> Arc<dyn MimeDetector> {
        self.inner
    }

    fn from_kind(kind: MimeDetectorKind) -> Self {
        Self::from_kind_with_config(kind, &MimeConfig::default())
    }

    fn from_kind_with_config(kind: MimeDetectorKind, config: &MimeConfig) -> Self {
        match kind {
            MimeDetectorKind::Repository => Self::new(Arc::new(
                RepositoryMimeDetector::from_mime_config(config.clone()),
            )),
            MimeDetectorKind::FileCommand => Self::new(Arc::new(
                FileCommandMimeDetector::from_mime_config(config.clone()),
            )),
        }
    }
}

impl Default for ArcMimeDetector {
    fn default() -> Self {
        Self::from_config(&MimeConfig::default())
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

    fn detect_reader(
        &self,
        reader: &mut dyn ReadSeek,
        filename: Option<&str>,
        policy: MimeDetectionPolicy,
    ) -> MimeResult<Option<String>> {
        self.inner.detect_reader(reader, filename, policy)
    }

    fn detect_file(&self, file: &Path, policy: MimeDetectionPolicy) -> MimeResult<Option<String>> {
        self.inner.detect_file(file, policy)
    }
}
