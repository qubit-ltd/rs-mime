// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! File-backed MIME detector helpers.

use std::fmt::Debug;
use std::io::Write;
use std::path::Path;

use qubit_local_files::{
    LocalFileSystem,
    LocalTempFileOptions,
};

use crate::{
    MimeDetectorCore,
    MimeError,
    MimeResult,
    StreamBasedMimeDetector,
};

/// Core implementation contract for detectors that only inspect local files.
pub trait FileBasedMimeDetector: Debug + Send + Sync {
    /// Gets the shared detector core.
    ///
    /// # Returns
    /// Shared detector configuration and merge/refinement behavior.
    fn core(&self) -> &MimeDetectorCore;

    /// Gets the maximum number of bytes needed for content inspection.
    ///
    /// # Returns
    /// Content prefix length to stage for byte and reader inputs.
    fn max_test_bytes(&self) -> usize;

    /// Guesses MIME type names from a filename.
    ///
    /// # Parameters
    /// - `filename`: File path or basename.
    ///
    /// # Returns
    /// Candidate MIME type names ordered by backend confidence.
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
}

impl<T> StreamBasedMimeDetector for T
where
    T: FileBasedMimeDetector,
{
    /// Gets the shared detector core.
    fn core(&self) -> &MimeDetectorCore {
        FileBasedMimeDetector::core(self)
    }

    /// Gets the maximum content prefix length needed by this detector.
    fn max_test_bytes(&self) -> usize {
        FileBasedMimeDetector::max_test_bytes(self)
    }

    /// Guesses MIME type names from filename rules.
    fn guess_from_filename(&self, filename: &str) -> Vec<String> {
        FileBasedMimeDetector::guess_from_filename(self, filename)
    }

    /// Stages content to a temporary local file before inspection.
    fn guess_from_content_bytes(
        &self,
        content: &[u8],
    ) -> MimeResult<Vec<String>> {
        with_temp_file(content, |path| {
            FileBasedMimeDetector::guess_from_local_file(self, path)
        })
    }

    /// Delegates local-file inspection to the file-based hook.
    fn guess_from_file_stream(
        &self,
        file: &Path,
    ) -> MimeResult<(Vec<String>, Vec<u8>)> {
        Ok((
            FileBasedMimeDetector::guess_from_local_file(self, file)?,
            Vec::new(),
        ))
    }
}

/// Stages content into a temporary file for file-based detectors.
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
pub(crate) fn with_temp_file<T>(
    content: &[u8],
    detect: impl FnOnce(&Path) -> MimeResult<T>,
) -> MimeResult<T> {
    let options = LocalTempFileOptions::new()
        .with_prefix("MimeDetectorTemp-")
        .with_suffix(".tmp");
    let mut file = LocalFileSystem::host()
        .create_temp_file(&options)
        .map_err(|error| MimeError::Io(error.into_io_error()))?;
    file.write_all(content)?;
    file.close();
    let result = detect(file.path());
    let cleanup = file.cleanup();
    match (result, cleanup) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(MimeError::Io(error.into_io_error())),
    }
}
