// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! State transferred to the worker while creating staged MIME input.

use qubit_operation::OperationContext;
use qubit_operation::ResourceRef;
use qubit_operation::ResourceReservation;

use super::super::Completer;
use super::super::MimeExecutionWorker;
use super::super::MimeResult;
use super::super::StagedMimeInput;
use super::CreateTempJob;

/// Carries temp-file creation state and completion back from the worker.
pub(in crate::managed_temp_staging) struct CreateStagedPayload {
    /// Operation context checked before and during staging.
    pub(in crate::managed_temp_staging) context: OperationContext,
    /// Reserved resources released if staging is rejected.
    pub(in crate::managed_temp_staging) reservation: ResourceReservation,
    /// Filesystem action run away from the async caller.
    pub(in crate::managed_temp_staging) create_temp: CreateTempJob,
    /// Worker responsible for running the filesystem action.
    pub(in crate::managed_temp_staging) worker: MimeExecutionWorker,
    /// Completion channel for the staged resource result.
    pub(in crate::managed_temp_staging) sender: Completer<MimeResult<ResourceRef<StagedMimeInput>>>,
    /// Optional test hook invoked around publication.
    #[cfg(test)]
    pub(in crate::managed_temp_staging) hook: Option<super::super::resource_identity::PublishHook>,
}
