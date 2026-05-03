/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Shared media stream classifier wrapper.
//!

use std::io::Read;
use std::ops::Deref;
use std::path::Path;
use std::sync::Arc;

use crate::{
    MediaStreamClassifier,
    MediaStreamType,
    MimeConfig,
    MimeResult,
};

use super::FfprobeCommandMediaStreamClassifier;
use super::media_stream_classifier_kind::MediaStreamClassifierKind;

/// A media stream classifier stored in an [`Arc`].
#[derive(Debug, Clone)]
pub struct ArcMediaStreamClassifier {
    inner: Arc<dyn MediaStreamClassifier>,
}

impl ArcMediaStreamClassifier {
    /// Wraps an existing shared media stream classifier.
    ///
    /// # Parameters
    /// - `classifier`: Classifier to wrap.
    ///
    /// # Returns
    /// Wrapped shared classifier.
    pub fn new(classifier: Arc<dyn MediaStreamClassifier>) -> Self {
        Self { inner: classifier }
    }

    /// Creates a shared classifier from an implementation name.
    ///
    /// # Parameters
    /// - `name`: Classifier selector.
    ///
    /// # Returns
    /// Matching classifier, or `None` when the selector is empty or unknown.
    pub fn from_name(name: &str) -> Option<Self> {
        MediaStreamClassifierKind::from_name(name).map(Self::from_kind)
    }

    /// Creates a shared classifier from MIME configuration.
    ///
    /// # Parameters
    /// - `config`: MIME configuration containing the default classifier selector.
    ///
    /// # Returns
    /// Configured classifier wrapper.
    pub fn from_config(config: &MimeConfig) -> Self {
        let backend = MediaStreamClassifierKind::select(config.media_stream_classifier_default());
        Self::from_kind(backend)
    }

    /// Unwraps this wrapper into the inner shared classifier.
    ///
    /// # Returns
    /// Inner shared classifier.
    pub fn into_inner(self) -> Arc<dyn MediaStreamClassifier> {
        self.inner
    }

    fn from_kind(kind: MediaStreamClassifierKind) -> Self {
        match kind {
            MediaStreamClassifierKind::FfprobeCommand => {
                Self::new(Arc::new(FfprobeCommandMediaStreamClassifier::new()))
            }
        }
    }
}

impl Default for ArcMediaStreamClassifier {
    fn default() -> Self {
        Self::from_config(&MimeConfig::default())
    }
}

impl Deref for ArcMediaStreamClassifier {
    type Target = dyn MediaStreamClassifier;

    fn deref(&self) -> &Self::Target {
        self.as_ref()
    }
}

impl AsRef<dyn MediaStreamClassifier> for ArcMediaStreamClassifier {
    fn as_ref(&self) -> &(dyn MediaStreamClassifier + 'static) {
        self.inner.as_ref()
    }
}

impl From<Arc<dyn MediaStreamClassifier>> for ArcMediaStreamClassifier {
    fn from(classifier: Arc<dyn MediaStreamClassifier>) -> Self {
        Self::new(classifier)
    }
}

impl From<ArcMediaStreamClassifier> for Arc<dyn MediaStreamClassifier> {
    fn from(classifier: ArcMediaStreamClassifier) -> Self {
        classifier.into_inner()
    }
}

impl MediaStreamClassifier for ArcMediaStreamClassifier {
    fn classify_file(&self, file: &Path) -> MimeResult<MediaStreamType> {
        self.inner.classify_file(file)
    }

    fn classify_reader(&self, reader: &mut dyn Read) -> MimeResult<MediaStreamType> {
        self.inner.classify_reader(reader)
    }
}
