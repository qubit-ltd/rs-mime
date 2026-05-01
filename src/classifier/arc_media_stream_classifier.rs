/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Shared media stream classifier wrapper.
//!
//! # Author
//!
//! Haixing Hu

use std::ops::Deref;
use std::path::Path;
use std::sync::Arc;

use crate::{
    ENV_MEDIA_STREAM_CLASSIFIER_DEFAULT, MediaStreamClassifier, MediaStreamType, MimeResult,
};

use super::FfprobeCommandMediaStreamClassifier;
use super::media_stream_classifier_backend::MediaStreamClassifierBackend;

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
        MediaStreamClassifierBackend::from_name(name).map(Self::from_backend)
    }

    /// Unwraps this wrapper into the inner shared classifier.
    ///
    /// # Returns
    /// Inner shared classifier.
    pub fn into_inner(self) -> Arc<dyn MediaStreamClassifier> {
        self.inner
    }

    fn from_backend(backend: MediaStreamClassifierBackend) -> Self {
        match backend {
            MediaStreamClassifierBackend::FfprobeCommand => {
                Self::new(Arc::new(FfprobeCommandMediaStreamClassifier::new()))
            }
        }
    }
}

impl Default for ArcMediaStreamClassifier {
    fn default() -> Self {
        let configured = std::env::var(ENV_MEDIA_STREAM_CLASSIFIER_DEFAULT).unwrap_or_default();
        let backend = MediaStreamClassifierBackend::select(&configured);
        Self::from_backend(backend)
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

    fn classify_content(&self, content: &[u8]) -> MimeResult<MediaStreamType> {
        self.inner.classify_content(content)
    }
}
