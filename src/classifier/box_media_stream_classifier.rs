/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Boxed media stream classifier wrapper.

use std::ops::Deref;
use std::path::Path;

use crate::{
    ENV_MEDIA_STREAM_CLASSIFIER_DEFAULT, MediaStreamClassifier, MediaStreamType, MimeResult,
};

use super::FfprobeCommandMediaStreamClassifier;
use super::media_stream_classifier_backend::MediaStreamClassifierBackend;

/// A media stream classifier stored in a [`Box`].
#[derive(Debug)]
pub struct BoxMediaStreamClassifier {
    inner: Box<dyn MediaStreamClassifier>,
}

impl BoxMediaStreamClassifier {
    /// Wraps an existing boxed media stream classifier.
    ///
    /// # Parameters
    /// - `classifier`: Classifier to wrap.
    ///
    /// # Returns
    /// Wrapped boxed classifier.
    pub fn new(classifier: Box<dyn MediaStreamClassifier>) -> Self {
        Self { inner: classifier }
    }

    /// Creates a boxed classifier from an implementation name.
    ///
    /// # Parameters
    /// - `name`: Classifier selector.
    ///
    /// # Returns
    /// Matching classifier, or `None` when the selector is empty or unknown.
    pub fn from_name(name: &str) -> Option<Self> {
        MediaStreamClassifierBackend::from_name(name).map(Self::from_backend)
    }

    /// Unwraps this wrapper into the inner boxed classifier.
    ///
    /// # Returns
    /// Inner boxed classifier.
    pub fn into_inner(self) -> Box<dyn MediaStreamClassifier> {
        self.inner
    }

    fn from_backend(backend: MediaStreamClassifierBackend) -> Self {
        match backend {
            MediaStreamClassifierBackend::FfprobeCommand => {
                Self::new(Box::new(FfprobeCommandMediaStreamClassifier::new()))
            }
        }
    }
}

impl Default for BoxMediaStreamClassifier {
    fn default() -> Self {
        let configured = std::env::var(ENV_MEDIA_STREAM_CLASSIFIER_DEFAULT).unwrap_or_default();
        let backend = MediaStreamClassifierBackend::select(&configured);
        Self::from_backend(backend)
    }
}

impl Deref for BoxMediaStreamClassifier {
    type Target = dyn MediaStreamClassifier;

    fn deref(&self) -> &Self::Target {
        self.as_ref()
    }
}

impl AsRef<dyn MediaStreamClassifier> for BoxMediaStreamClassifier {
    fn as_ref(&self) -> &(dyn MediaStreamClassifier + 'static) {
        self.inner.as_ref()
    }
}

impl From<Box<dyn MediaStreamClassifier>> for BoxMediaStreamClassifier {
    fn from(classifier: Box<dyn MediaStreamClassifier>) -> Self {
        Self::new(classifier)
    }
}

impl From<BoxMediaStreamClassifier> for Box<dyn MediaStreamClassifier> {
    fn from(classifier: BoxMediaStreamClassifier) -> Self {
        classifier.into_inner()
    }
}

impl MediaStreamClassifier for BoxMediaStreamClassifier {
    fn classify_file(&self, file: &Path) -> MimeResult<MediaStreamType> {
        self.inner.classify_file(file)
    }

    fn classify_content(&self, content: &[u8]) -> MimeResult<MediaStreamType> {
        self.inner.classify_content(content)
    }
}
