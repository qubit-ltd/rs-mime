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
use std::io::{
    Read,
    copy,
};
use std::path::Path;

use tempfile::Builder;

use crate::{
    MimeError,
    MimeResult,
};

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
/// Returns [`MimeError::Io`](crate::MimeError::Io) when the stream cannot be read or the
/// temporary file cannot be written, or returns the callback error when classification fails.
pub(crate) fn with_temp_reader<T>(
    reader: &mut dyn Read,
    classify: impl FnOnce(&Path) -> MimeResult<T>,
) -> MimeResult<T> {
    let mut file = Builder::new()
        .prefix("FileBasedMediaStreamClassifier-")
        .suffix(".tmp")
        .tempfile()?;
    copy(reader, file.as_file_mut())?;
    classify(file.path())
}
