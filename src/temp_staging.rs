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
///
/// # Type Parameters
/// - `T`: Value returned by the inspection callback.
///
/// # Parameters
/// - `prefix`: Prefix used to identify the temporary filename.
/// - `stage`: Callback that writes the payload to the temporary file.
/// - `inspect`: Callback that examines the closed temporary file by path.
///
/// # Returns
/// The value returned by `inspect` when staging, inspection, and cleanup
/// succeed.
///
/// # Errors
/// Returns `MimeError::Io` when temporary-file creation or cleanup alone fails.
/// Propagates the staging or inspection error when cleanup succeeds. If
/// cleanup also fails, returns `MimeError::TemporaryCleanup` with the primary
/// error and cleanup context.
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

#[cfg(test)]
mod tests {
    use super::with_temp_file;
    use crate::MimeError;

    #[test]
    fn test_temp_creation_failure_is_mapped_before_staging_or_inspection() {
        let staged = std::cell::Cell::new(false);
        let inspected = std::cell::Cell::new(false);
        let error = with_temp_file::<()>(
            "invalid/component/",
            |_| {
                staged.set(true);
                Ok(())
            },
            |_| {
                inspected.set(true);
                Ok(())
            },
        )
        .expect_err("a path separator in the filename prefix must prevent temp creation");

        assert!(matches!(error, MimeError::Io(_)));
        assert!(!staged.get(), "staging must not run without a created owner");
        assert!(!inspected.get(), "inspection must not run without a created owner");
    }

    #[test]
    fn test_inspection_panic_unwinds_after_best_effort_temp_cleanup() {
        let mut staged_path = None;
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = with_temp_file::<()>(
                "InspectionPanic-",
                |file| {
                    staged_path = Some(file.path().to_path_buf());
                    Ok(())
                },
                |_| panic!("inspection panic for cleanup test"),
            );
        }));

        assert!(panic.is_err(), "inspection panic must keep unwinding");
        let path = staged_path.expect("staging callback should have recorded the temp path");
        assert!(!path.exists(), "temporary file guard should clean up during unwind");
        assert!(!path.parent().expect("temp file path has a parent").exists());
    }
}
