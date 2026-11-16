// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Shared resource control for a staged MIME input.

use std::sync::Arc;
use std::sync::Mutex;

use qubit_local_files::LocalTempFile;
use qubit_operation::ResourceId;

use crate::MimeError;
use crate::MimeResult;
use crate::command_execution::MimeExecutionWorker;
use crate::detector::StagingPermit;

/// Shared control port for a staged file. Its resource owner is registered
/// exactly once by operation; consumers receive only `ResourceRef`s.
pub(crate) struct StagedMimeInput {
    /// Native temporary-file owner retained until cleanup succeeds.
    pub(super) owner: Mutex<LocalTempFile>,
    /// Permits held until the native file is released.
    pub(super) permits: Mutex<Vec<StagingPermit>>,
    /// Worker that performs writes and cleanup.
    pub(super) worker: MimeExecutionWorker,
    /// Resource identity shared with its cleanup adapter.
    pub(super) cleanup_id: Arc<Mutex<Option<ResourceId>>>,
}

impl StagedMimeInput {
    #[cfg(test)]
    /// Builds a staged-file test resource around an existing owner.
    ///
    /// # Parameters
    /// - `owner`: Native temporary file used as the staged resource.
    /// - `permit`: Staging budget held by the resource.
    /// - `worker`: Worker used for cleanup operations.
    ///
    /// # Returns
    /// A staged input retaining the owner and budget permit.
    pub(crate) fn new(owner: LocalTempFile, permit: StagingPermit, worker: MimeExecutionWorker) -> Self {
        Self {
            owner: Mutex::new(owner),
            permits: Mutex::new(vec![permit]),
            worker,
            cleanup_id: Arc::new(Mutex::new(None)),
        }
    }

    /// Returns a cloned native path for command argument construction.
    pub(crate) fn path(&self) -> MimeResult<std::path::PathBuf> {
        let owner = self.owner.lock().unwrap_or_else(|poison| poison.into_inner());
        Ok(owner.path().to_path_buf())
    }

    /// Writes bytes and retains their staging permit for the resource lifetime.
    ///
    /// # Parameters
    /// - `bytes`: Chunk to write to the temporary file.
    /// - `permit`: Budget permit retained until cleanup.
    ///
    /// # Returns
    /// `Ok(())` after the full chunk is written.
    ///
    /// # Errors
    /// Returns an I/O error when the underlying file write fails.
    pub(super) fn write_chunk(&self, bytes: &[u8], permit: StagingPermit) -> MimeResult<()> {
        // Store the permit before writing: if a partial write fails, the
        // registered resource continues to account for every reserved byte.
        self.permits.lock().unwrap_or_else(|p| p.into_inner()).push(permit);
        use std::io::Write;
        let mut owner = self.owner.lock().unwrap_or_else(|p| p.into_inner());
        owner
            .as_file_mut()
            .and_then(|file| file.write_all(bytes))
            .map_err(|error| MimeError::Io(error.into()))
    }
}
