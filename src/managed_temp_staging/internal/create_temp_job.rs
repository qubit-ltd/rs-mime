// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Filesystem action used to create a staged MIME input.

use qubit_local_files::LocalFileError;
use qubit_local_files::LocalTempFile;

/// A sendable action that creates a local temporary file for staging.
///
/// The closure runs on the managed worker so filesystem creation does not block
/// the caller's asynchronous poll.
pub(in crate::managed_temp_staging) type CreateTempJob =
    Box<dyn FnOnce() -> Result<LocalTempFile, Box<LocalFileError>> + Send + 'static>;
