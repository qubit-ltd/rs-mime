// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Shared media stream classifier helpers.

use std::fs;
use std::io::ErrorKind;
use std::io::Read;
use std::io::Write;
use std::path::Path;

use qubit_io::Streams;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::LocalTempFileOptions;

use crate::MimeError;
use crate::MimeResult;
use crate::constants::DEFAULT_TEMP_NAME_MAX_ATTEMPTS;

/// Validates that a path is a readable local file.
///
/// # Parameters
/// - `path`: Path to validate.
///
/// # Errors
/// Returns [`MimeError::Io`](crate::MimeError::Io) when metadata cannot be read
/// or the file cannot be opened, and
/// [`MimeError::InvalidClassifierInput`](crate::MimeError::InvalidClassifierInput)
/// when the path is not a regular file.
pub(crate) fn validate_readable_file(path: &Path) -> MimeResult<()> {
    let metadata = fs::metadata(path)?;
    if !metadata.is_file() {
        return Err(MimeError::invalid_classifier_input(format!(
            "path is not a regular file: {}",
            path.display()
        )));
    }
    let reader = fs::File::open(path)?;
    drop(reader);
    Ok(())
}

/// Stages a reader into a temporary file and calls `classify`.
///
/// # Parameters
/// - `reader`: Stream whose content should be staged.
/// - `max_staging_size`: Maximum accepted stream size in bytes.
/// - `classify`: Callback receiving the temporary local file path.
///
/// # Returns
/// The callback result.
///
/// # Errors
/// Returns [`MimeError::Io`](crate::MimeError::Io) when the stream cannot be
/// read or the temporary file cannot be written. Returns
/// [`MimeError::InvalidClassifierInput`](crate::MimeError::InvalidClassifierInput) when the
/// stream exceeds `max_staging_size`, or returns the callback error when
/// classification fails.
pub(crate) fn with_temp_reader<T>(
    reader: &mut dyn Read,
    max_staging_size: u64,
    classify: impl FnOnce(&Path) -> MimeResult<T>,
) -> MimeResult<T> {
    let options = LocalTempFileOptions::new()
        .with_parent(&std::env::temp_dir())
        .with_prefix("FileBasedMediaStreamClassifier-")
        .with_suffix(".tmp")
        .with_max_attempts(DEFAULT_TEMP_NAME_MAX_ATTEMPTS)
        .with_create_parent();
    let filesystem = LocalFileSystem::host()
        .map_err(|error| MimeError::Io(error.into_io_error()))?;
    let mut file = filesystem
        .create_temp_file_with_options(&options)
        .map_err(|error| MimeError::Io(error.into_io_error()))?;
    copy_to_temp_file(reader, &mut file, max_staging_size)?;
    file.close();
    let result = classify(file.path());
    let cleanup = file.cleanup();
    match (result, cleanup) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(MimeError::Io(error.into_io_error())),
    }
}

/// Copies a reader into a temporary file while enforcing a byte limit.
///
/// # Parameters
/// - `reader`: Source stream.
/// - `writer`: Temporary file writer.
/// - `max_staging_size`: Maximum accepted stream size in bytes.
///
/// # Errors
/// Returns [`MimeError::InvalidClassifierInput`](crate::MimeError::InvalidClassifierInput) when
/// the stream exceeds `max_staging_size`, or
/// [`MimeError::Io`](crate::MimeError::Io) for I/O failures.
fn copy_to_temp_file(
    reader: &mut dyn Read,
    writer: &mut dyn Write,
    max_staging_size: u64,
) -> MimeResult<()> {
    Streams::copy_to_end_limited(reader, writer, max_staging_size)
        .map(|_| ())
        .map_err(|error| {
            if error.kind() == ErrorKind::InvalidData {
                MimeError::invalid_classifier_input(format!(
                    "media stream input exceeds staging limit of {max_staging_size} bytes"
                ))
            } else {
                error.into()
            }
        })
}
