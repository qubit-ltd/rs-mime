// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! File-backed media stream classifier helpers.

use std::fmt::Debug;
use std::io::Read;
use std::path::Path;

use super::media_stream_classifier_helpers::with_temp_reader;
use crate::DEFAULT_MEDIA_STREAM_MAX_STAGING_SIZE;
use crate::MediaStreamClassifierBackend;
use crate::MediaStreamType;
use crate::MimeResult;

/// Core implementation contract for classifiers that only operate on local
/// files.
pub trait FileBasedMediaStreamClassifier: Debug + Send + Sync {
    /// Gets the maximum bytes staged from reader/content input.
    ///
    /// # Returns
    /// Maximum accepted stream size before staging is rejected.
    fn max_staging_size(&self) -> u64 {
        DEFAULT_MEDIA_STREAM_MAX_STAGING_SIZE
    }

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
}

impl<T> MediaStreamClassifierBackend for T
where
    T: FileBasedMediaStreamClassifier,
{
    /// Delegates local-file classification to the file-based hook.
    fn classify_by_local_file(
        &self,
        file: &Path,
    ) -> MimeResult<MediaStreamType> {
        FileBasedMediaStreamClassifier::classify_by_local_file(self, file)
    }

    /// Stages stream content to a temporary local file before classification.
    fn classify_by_content(
        &self,
        reader: &mut dyn Read,
    ) -> MimeResult<MediaStreamType> {
        with_temp_reader(reader, self.max_staging_size(), |path| {
            FileBasedMediaStreamClassifier::classify_by_local_file(self, path)
        })
    }
}
