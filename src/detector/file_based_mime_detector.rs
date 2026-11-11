// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! File-backed MIME detector helpers.

use std::fmt::Debug;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;

use qubit_io::Output;

use crate::MimeDetectorCore;
use crate::MimeResult;
use crate::StreamBasedMimeDetector;

/// Core implementation contract for detectors that only inspect local files.
///
/// # Examples
///
/// ```
/// use qubit_mime::FileBasedMimeDetector;
/// use qubit_mime::FileCommandMimeDetector;
///
/// let detector = FileCommandMimeDetector::new();
/// let candidates = detector.guess_from_filename("document.pdf");
/// assert!(!candidates.is_empty());
/// ```
pub trait FileBasedMimeDetector: Debug + Send + Sync {
    /// Gets the shared detector core.
    ///
    /// # Returns
    /// Shared detector configuration and merge/refinement behavior.
    #[must_use]
    fn core(&self) -> &MimeDetectorCore;

    /// Gets the maximum number of bytes needed for content inspection.
    ///
    /// # Returns
    /// Content prefix length to stage for byte and reader inputs.
    #[must_use]
    fn max_test_bytes(&self) -> usize;

    /// Guesses MIME type names from a filename.
    ///
    /// # Parameters
    /// - `filename`: File path or basename.
    ///
    /// # Returns
    /// Candidate MIME type names ordered by backend confidence.
    #[must_use]
    fn guess_from_filename(&self, filename: &str) -> Vec<String>;

    /// Guesses MIME type names from one local file.
    ///
    /// # Parameters
    /// - `file`: Local file path readable by the backend.
    ///
    /// # Returns
    /// Candidate MIME type names ordered by backend confidence.
    ///
    /// # Errors
    /// Returns an error when local-file inspection fails.
    fn guess_from_local_file(&self, file: &Path) -> MimeResult<Vec<String>>;

    /// Stages and inspects content asynchronously. The compatibility default
    /// performs the existing synchronous staging inline; implementations with
    /// blocking work should override this hook.
    ///
    /// # Parameters
    /// - `content`: Content bytes to stage and inspect.
    ///
    /// # Returns
    /// A future containing candidate MIME type names.
    ///
    /// # Errors
    /// Propagates staging and local-file inspection errors.
    fn guess_from_staged_content_async<'a>(
        &'a self,
        content: &'a [u8],
    ) -> Pin<Box<dyn Future<Output = MimeResult<Vec<String>>> + Send + 'a>> {
        Box::pin(async move { with_temp_file(content, |path| Self::guess_from_local_file(self, path)) })
    }
}

impl<T> StreamBasedMimeDetector for T
where
    T: FileBasedMimeDetector,
{
    /// Gets the shared detector core.
    #[inline]
    fn core(&self) -> &MimeDetectorCore {
        FileBasedMimeDetector::core(self)
    }

    /// Gets the maximum content prefix length needed by this detector.
    #[inline]
    fn max_test_bytes(&self) -> usize {
        FileBasedMimeDetector::max_test_bytes(self)
    }

    /// Guesses MIME type names from filename rules.
    ///
    /// # Parameters
    /// - `filename`: File path or basename to match.
    #[inline]
    fn guess_from_filename(&self, filename: &str) -> Vec<String> {
        FileBasedMimeDetector::guess_from_filename(self, filename)
    }

    /// Stages content to a temporary local file before inspection.
    ///
    /// # Parameters
    /// - `content`: Content bytes staged for file-based inspection.
    fn guess_from_content_bytes(&self, content: &[u8]) -> MimeResult<Vec<String>> {
        with_temp_file(content, |path| FileBasedMimeDetector::guess_from_local_file(self, path))
    }

    /// Stages and inspects content using the file-based asynchronous hook.
    ///
    /// # Parameters
    /// - `content`: Content bytes passed through to the file-based hook.
    ///
    /// # Returns
    /// A future containing candidate MIME type names.
    ///
    /// # Errors
    /// Propagates staging and file inspection errors.
    fn guess_from_content_bytes_async<'a>(
        &'a self,
        content: &'a [u8],
    ) -> Pin<Box<dyn Future<Output = MimeResult<Vec<String>>> + Send + 'a>> {
        FileBasedMimeDetector::guess_from_staged_content_async(self, content)
    }

    /// Delegates local-file inspection to the file-based hook.
    ///
    /// # Parameters
    /// - `file`: Local file path passed to the file-based detector.
    fn guess_from_file_stream(&self, file: &Path) -> MimeResult<(Vec<String>, Vec<u8>)> {
        Ok((FileBasedMimeDetector::guess_from_local_file(self, file)?, Vec::new()))
    }
}

/// Stages content into a temporary file for file-based detectors.
///
/// # Type Parameters
/// - `T`: Value returned by the detector callback.
///
/// # Parameters
/// - `content`: Content bytes to stage.
/// - `detect`: Callback receiving the temporary path.
///
/// # Returns
/// The callback result.
///
/// # Errors
/// Returns [`MimeError::Io`](crate::MimeError::Io) when the temporary file
/// cannot be written.
pub(crate) fn with_temp_file<T>(content: &[u8], detect: impl FnOnce(&Path) -> MimeResult<T>) -> MimeResult<T> {
    crate::temp_staging::with_temp_file(
        "MimeDetectorTemp-",
        |file| Output::write_fully(file, content).map_err(Into::into),
        detect,
    )
}
