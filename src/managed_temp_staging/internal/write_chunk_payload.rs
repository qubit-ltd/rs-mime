// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! State transferred to the worker while writing staged MIME bytes.

use qubit_operation::OperationContext;
use qubit_operation::ResourceRef;

use super::super::Completer;
use super::super::MimeResult;
use super::super::OwnedMemoryPermit;
use super::super::StagedMimeInput;
use super::super::StagingPermit;

/// Carries one staged write and its memory permits to the worker.
pub(in crate::managed_temp_staging) struct WriteChunkPayload {
    /// Operation context used to stop writes after cancellation.
    pub(in crate::managed_temp_staging) context: OperationContext,
    /// Staged file receiving these bytes.
    pub(in crate::managed_temp_staging) staged: ResourceRef<StagedMimeInput>,
    /// Owned bytes to append to the staged file.
    pub(in crate::managed_temp_staging) bytes: Vec<u8>,
    /// Budget permit for the incoming chunk.
    pub(in crate::managed_temp_staging) permit: StagingPermit,
    /// Owned-memory permit held until the write completes.
    pub(in crate::managed_temp_staging) owned_permit: OwnedMemoryPermit,
    /// Completion channel for the write result.
    pub(in crate::managed_temp_staging) sender: Completer<MimeResult<()>>,
}
