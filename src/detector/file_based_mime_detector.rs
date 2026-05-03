/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! File-backed MIME detector helpers.

use std::fmt::Debug;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::{MimeDetectorBackend, MimeDetectorCore, MimeResult};

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

impl<T> MimeDetectorBackend for T
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
    fn guess_from_content(&self, content: &[u8]) -> MimeResult<Vec<String>> {
        with_temp_file(content, |path| {
            FileBasedMimeDetector::guess_from_local_file(self, path)
        })
    }

    /// Delegates local-file inspection to the file-based hook.
    fn guess_from_file(&self, file: &Path) -> MimeResult<(Vec<String>, Vec<u8>)> {
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
/// Returns [`MimeError::Io`](crate::MimeError::Io) when the temporary file cannot be written.
pub(crate) fn with_temp_file<T>(
    content: &[u8],
    detect: impl FnOnce(&PathBuf) -> MimeResult<T>,
) -> MimeResult<T> {
    let path = unique_temp_path("MimeDetectorTemp", ".tmp");
    fs::write(&path, content)?;
    let result = detect(&path);
    let _ = fs::remove_file(&path);
    result
}

/// Builds a best-effort unique temporary path.
///
/// # Parameters
/// - `prefix`: Filename prefix.
/// - `suffix`: Filename suffix.
///
/// # Returns
/// Path under the OS temporary directory.
fn unique_temp_path(prefix: &str, suffix: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("{prefix}-{}-{counter}{suffix}", std::process::id()))
}
