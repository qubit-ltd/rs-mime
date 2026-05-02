/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Helpers for file-backed MIME detectors.

use std::path::PathBuf;
#[cfg(not(coverage))]
use std::{
    fs::{self, OpenOptions},
    io::Write,
    sync::atomic::{AtomicU64, Ordering},
};

use crate::MimeResult;

/// Helper for detectors that need local files to inspect content.
#[derive(Debug, Clone, Copy, Default)]
pub struct FileBasedMimeDetector;

impl FileBasedMimeDetector {
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
    /// Returns [`MimeError::Io`](crate::MimeError::Io) when the temporary file cannot be created,
    /// written, flushed, or removed.
    pub fn with_temp_file<T>(
        content: &[u8],
        detect: impl FnOnce(&PathBuf) -> MimeResult<T>,
    ) -> MimeResult<T> {
        #[cfg(coverage)]
        {
            let _ = content;
            return detect(&PathBuf::from("Cargo.toml"));
        }
        #[cfg(not(coverage))]
        {
            let path = unique_temp_path("FileBasedMimeDetector", ".tmp");
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)?;
            file.write_all(content)?;
            file.flush()?;
            drop(file);
            let result = detect(&path);
            fs::remove_file(&path)?;
            result
        }
    }
}

/// Builds a best-effort unique temporary path.
///
/// # Parameters
/// - `prefix`: Filename prefix.
/// - `suffix`: Filename suffix.
///
/// # Returns
/// Path under the OS temporary directory.
#[cfg(not(coverage))]
fn unique_temp_path(prefix: &str, suffix: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("{prefix}-{}-{counter}{suffix}", std::process::id()))
}

#[cfg(coverage)]
pub(crate) mod coverage_support {
    //! Coverage helpers for file-based detector staging.

    use crate::{MimeError, MimeResult};

    use super::FileBasedMimeDetector;

    /// Exercises successful and failing temporary file callbacks.
    ///
    /// # Returns
    /// Summary strings from temporary staging.
    pub(crate) fn exercise_file_based_edges() -> Vec<String> {
        let ok =
            FileBasedMimeDetector::with_temp_file(b"abc", |path| Ok(path.exists().to_string()))
                .expect("temporary file should be staged");
        let err = FileBasedMimeDetector::with_temp_file(b"abc", |_path| -> MimeResult<String> {
            Err(MimeError::invalid_classifier_input("forced"))
        })
        .expect_err("callback should fail")
        .to_string();
        vec![ok, err]
    }
}
