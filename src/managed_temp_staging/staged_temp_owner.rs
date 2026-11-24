// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Native file or recovery owner retained by staged MIME resources.

use qubit_local_files::LocalTempFile;
use qubit_local_files::LocalTempRecovery;

/// Cleanup responsibility retained by an operation-owned staging resource.
pub(super) enum StagedTempOwner {
    /// A temporary file successfully created for staging.
    File {
        /// Native temporary-file owner retained until cleanup succeeds.
        owner: LocalTempFile,
    },
    /// Native cleanup authority returned with a failed temp creation.
    Recovery {
        /// Native recovery owner retained until cleanup succeeds.
        owner: LocalTempRecovery,
    },
}

impl StagedTempOwner {
    /// Returns the path associated with the owned resource.
    pub(super) fn path(&self) -> &std::path::Path {
        match self {
            Self::File { owner } => owner.path(),
            Self::Recovery { owner } => owner.path(),
        }
    }

    /// Writes to a staged file, rejecting recovery-only owners.
    pub(super) fn as_file_mut(&mut self) -> std::io::Result<&mut std::fs::File> {
        match self {
            Self::File { owner } => owner.as_file_mut(),
            Self::Recovery { .. } => Err(std::io::Error::other("temporary recovery owner is not a staged file")),
        }
    }

    /// Attempts cleanup while retaining this owner on failure.
    pub(super) fn cleanup(&mut self) -> std::io::Result<()> {
        match self {
            Self::File { owner } => owner.cleanup().map_err(|error| error.into_io_error()),
            Self::Recovery { owner } => owner.cleanup().map_err(|error| error.into_io_error()),
        }
    }

    /// Closes the active file handle when this owner is a staged file.
    #[cfg(test)]
    pub(super) fn close(&mut self) {
        if let Self::File { owner } = self {
            owner.close();
        }
    }
}
