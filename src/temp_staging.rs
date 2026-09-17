// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Shared temporary-file lifecycle for file-backed MIME operations.

use std::path::Path;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::LocalTempFile;
use qubit_local_files::options::LocalTempFileOptions;

use crate::MimeError;
use crate::MimeResult;
use crate::constants::DEFAULT_TEMP_NAME_MAX_ATTEMPTS;

/// Stages input, closes its handle, inspects the path, and explicitly cleans
/// up.
///
/// `prefix` identifies the caller's temporary filename. `stage` writes the
/// payload; `inspect` runs only after successful staging and handle closure.
/// Returns the inspection result when cleanup succeeds. A sole cleanup failure
/// becomes `MimeError::Io`; simultaneous failures retain the primary error and
/// typed cleanup context in `MimeError::TemporaryCleanup`. Panics still unwind
/// through the temporary guard's best-effort Drop cleanup.
pub(crate) fn with_temp_file<T>(
    prefix: &str,
    stage: impl FnOnce(&mut LocalTempFile) -> MimeResult<()>,
    inspect: impl FnOnce(&Path) -> MimeResult<T>,
) -> MimeResult<T> {
    let options = LocalTempFileOptions::new()
        .with_parent(&std::env::temp_dir())
        .with_prefix(prefix)
        .with_suffix(".tmp")
        .with_max_attempts(DEFAULT_TEMP_NAME_MAX_ATTEMPTS)
        .with_create_parent();
    let filesystem = LocalFileSystem::host().map_err(|error| MimeError::Io(error.into_io_error()))?;
    let mut file = filesystem
        .create_temp_file_with_options(&options)
        .map_err(|error| MimeError::Io(error.into_io_error()))?;
    let staged = stage(&mut file);
    file.close();
    let result = staged.and_then(|()| inspect(file.path()));
    match (result, file.cleanup()) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(primary), Ok(())) => Err(primary),
        (Ok(_), Err(cleanup)) => Err(MimeError::Io(cleanup.into_io_error())),
        (Err(primary), Err(cleanup)) => Err(MimeError::TemporaryCleanup {
            primary: Box::new(primary),
            cleanup: Box::new(cleanup),
        }),
    }
}
