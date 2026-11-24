// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Private payload state passed to the managed cleanup worker.

use qubit_operation::CleanupOutcome;
use qubit_operation::ResourceId;

use super::super::super::completion::Completer;
use super::super::StagedMimeInput;

/// Transfers staged ownership and its cleanup response to the worker.
pub(in crate::managed_temp_staging::managed_temp_cleanup) struct CleanupDispatchPayload {
    /// Staged resource whose native temporary file must be removed.
    pub(in crate::managed_temp_staging::managed_temp_cleanup) resource: StagedMimeInput,
    /// Stable identity used to report cleanup failures.
    pub(in crate::managed_temp_staging::managed_temp_cleanup) resource_id: ResourceId,
    /// Completion channel that receives the cleanup outcome.
    pub(in crate::managed_temp_staging::managed_temp_cleanup) sender: Completer<CleanupOutcome<StagedMimeInput>>,
}
