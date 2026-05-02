/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! File-backed media stream classifier helpers.

use std::fmt::Debug;
use std::io::Read;
use std::path::Path;

use crate::{MediaStreamClassifierBackend, MediaStreamType, MimeResult};

use super::media_stream_classifier_helpers::with_temp_reader;

/// Core implementation contract for classifiers that only operate on local files.
pub trait FileBasedMediaStreamClassifier: Debug + Send + Sync {
    /// Classifies one validated local file.
    ///
    /// # Parameters
    /// - `file`: Readable local media file.
    ///
    /// # Returns
    /// Media stream classification.
    ///
    /// # Errors
    /// Returns [`MimeError::Io`](crate::MimeError::Io) or another [`MimeError`](crate::MimeError)
    /// when backend classification fails.
    fn classify_by_local_file(&self, file: &Path) -> MimeResult<MediaStreamType>;
}

impl<T> MediaStreamClassifierBackend for T
where
    T: FileBasedMediaStreamClassifier,
{
    /// Delegates local-file classification to the file-based hook.
    fn classify_by_local_file(&self, file: &Path) -> MimeResult<MediaStreamType> {
        FileBasedMediaStreamClassifier::classify_by_local_file(self, file)
    }

    /// Stages stream content to a temporary local file before classification.
    fn classify_by_content(&self, reader: &mut dyn Read) -> MimeResult<MediaStreamType> {
        with_temp_reader(reader, |path| {
            FileBasedMediaStreamClassifier::classify_by_local_file(self, path)
        })
    }
}

#[cfg(coverage)]
pub(crate) mod coverage_support {
    //! Coverage helpers for file-based classifier staging.

    use std::path::Path;

    use crate::{MediaStreamClassifier, MediaStreamType, MimeError, MimeResult};

    use super::FileBasedMediaStreamClassifier;

    #[derive(Debug)]
    struct CoverageClassifier;

    impl FileBasedMediaStreamClassifier for CoverageClassifier {
        /// Returns whether the staged file exists.
        fn classify_by_local_file(&self, path: &Path) -> MimeResult<MediaStreamType> {
            if path.exists() {
                Ok(MediaStreamType::VideoOnly)
            } else {
                Err(MimeError::invalid_classifier_input("missing staged file"))
            }
        }
    }

    #[derive(Debug)]
    struct FailingCoverageClassifier;

    impl FileBasedMediaStreamClassifier for FailingCoverageClassifier {
        /// Always fails local-file classification.
        fn classify_by_local_file(&self, _path: &Path) -> MimeResult<MediaStreamType> {
            Err(MimeError::invalid_classifier_input("forced"))
        }
    }

    /// Exercises successful and failing temporary file callbacks.
    ///
    /// # Returns
    /// Summary strings from temporary staging.
    pub(crate) fn exercise_file_based_classifier_edges() -> Vec<String> {
        let ok = CoverageClassifier
            .classify_content(b"abc")
            .expect("temporary file should be staged");
        let err = FailingCoverageClassifier
            .classify_content(b"abc")
            .expect_err("callback should fail")
            .to_string();
        vec![format!("{ok:?}"), err]
    }
}
