// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Core media stream classifier backend contract.

use std::fmt::Debug;
use std::io::Read;
use std::path::Path;

use crate::MimeResult;

use super::MediaStreamClassifier;
use super::MediaStreamType;
use super::media_stream_classifier_helpers::validate_readable_file;

/// Core implementation contract for classifiers that can handle local files and
/// streams.
pub trait MediaStreamClassifierBackend: Debug + Send + Sync {
    /// Classifies one validated local file.
    ///
    /// # Parameters
    /// - `file`: Readable local media file.
    ///
    /// # Returns
    /// Media stream classification.
    ///
    /// # Errors
    /// Returns [`MimeError::Io`](crate::MimeError::Io) or another
    /// [`MimeError`](crate::MimeError) when backend classification fails.
    fn classify_by_local_file(
        &self,
        file: &Path,
    ) -> MimeResult<MediaStreamType>;

    /// Classifies media bytes from a stream.
    ///
    /// # Parameters
    /// - `reader`: Media stream to classify.
    ///
    /// # Returns
    /// Media stream classification.
    ///
    /// # Errors
    /// Returns [`MimeError::Io`](crate::MimeError::Io) or another
    /// [`MimeError`](crate::MimeError) when backend classification fails.
    fn classify_by_content(
        &self,
        reader: &mut dyn Read,
    ) -> MimeResult<MediaStreamType>;
}

impl<T> MediaStreamClassifier for T
where
    T: MediaStreamClassifierBackend,
{
    /// Validates the local file and delegates to the backend hook.
    fn classify_file(&self, file: &Path) -> MimeResult<MediaStreamType> {
        validate_readable_file(file)?;
        self.classify_by_local_file(file)
    }

    /// Delegates stream classification to the backend hook.
    fn classify_reader(
        &self,
        reader: &mut dyn Read,
    ) -> MimeResult<MediaStreamType> {
        self.classify_by_content(reader)
    }
}
