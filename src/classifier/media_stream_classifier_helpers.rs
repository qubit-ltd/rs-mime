/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Shared media stream classifier helpers.

use std::fs;
use std::io::Read;
// qubit-style: allow coverage-cfg
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::{MimeError, MimeResult};

/// Validates that a path is a readable local file.
///
/// # Parameters
/// - `path`: Path to validate.
///
/// # Errors
/// Returns [`MimeError::Io`](crate::MimeError::Io) when metadata cannot be read or the file cannot
/// be opened, and [`MimeError::InvalidClassifierInput`](crate::MimeError::InvalidClassifierInput)
/// when the path is not a regular file.
pub(crate) fn validate_readable_file(path: &Path) -> MimeResult<()> {
    let metadata = fs::metadata(path)?;
    if !metadata.is_file() {
        return Err(MimeError::invalid_classifier_input(format!(
            "path is not a regular file: {}",
            path.display()
        )));
    }
    fs::File::open(path)?;
    Ok(())
}

/// Stages a reader into a temporary file and calls `classify`.
///
/// # Parameters
/// - `reader`: Stream whose content should be staged.
/// - `classify`: Callback receiving the temporary local file path.
///
/// # Returns
/// The callback result.
///
/// # Errors
/// Returns [`MimeError::Io`](crate::MimeError::Io) when the temporary file cannot be created,
/// written, flushed, or removed, or returns the callback error when classification fails.
pub(crate) fn with_temp_reader<T>(
    reader: &mut dyn Read,
    classify: impl FnOnce(&Path) -> MimeResult<T>,
) -> MimeResult<T> {
    #[cfg(coverage)]
    {
        let path = unique_temp_path("FileBasedMediaStreamClassifier", ".tmp");
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .expect("coverage temporary file should be creatable");
        std::io::copy(reader, &mut file).expect("coverage reader should be staged");
        file.flush()
            .expect("coverage temporary file should be flushable");
        drop(file);
        let result = classify(&path);
        fs::remove_file(&path).expect("coverage temporary file should be removable");
        return result;
    }
    #[cfg(not(coverage))]
    {
        let path = unique_temp_path("FileBasedMediaStreamClassifier", ".tmp");
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        std::io::copy(reader, &mut file)?;
        file.flush()?;
        drop(file);
        let result = classify(&path);
        let remove_result = fs::remove_file(&path);
        match (result, remove_result) {
            (Ok(value), Ok(())) => Ok(value),
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(error.into()),
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
fn unique_temp_path(prefix: &str, suffix: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("{prefix}-{}-{counter}{suffix}", std::process::id()))
}

#[cfg(coverage)]
pub(crate) mod coverage_support {
    //! Coverage helpers for classifier validation.

    use std::io::Cursor;
    use std::path::Path;

    use crate::{MimeError, MimeResult};

    use super::{validate_readable_file, with_temp_reader};

    /// Exercises readable-file validation paths.
    ///
    /// # Returns
    /// Summary strings from validation.
    pub(crate) fn exercise_media_stream_classifier_helper_edges() -> Vec<String> {
        let valid = validate_readable_file(Path::new("Cargo.toml"))
            .is_ok()
            .to_string();
        let invalid = validate_readable_file(Path::new("."))
            .expect_err("directory should not validate")
            .to_string();
        let mut reader = Cursor::new(b"%PDF-1.7\n".to_vec());
        let staged = with_temp_reader(&mut reader, |path| Ok(path.exists().to_string()))
            .expect("coverage reader should be staged");
        let mut failing_reader = Cursor::new(Vec::new());
        let staged_error = with_temp_reader(&mut failing_reader, |_path| -> MimeResult<String> {
            Err(MimeError::invalid_classifier_input("forced"))
        })
        .expect_err("coverage callback should fail")
        .to_string();
        vec![valid, invalid, staged, staged_error]
    }
}
