// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Payload retained by a queued MIME command worker job.

use std::sync::Arc;

use qubit_command::Command;
use qubit_command::CommandCancellation;
use qubit_command::CommandRunner;
use qubit_operation::OperationContext;
use qubit_operation::ResourceRef;

use super::PreparedCommand;
use crate::managed_temp_staging::StagedMimeInput;

/// Owns the command inputs and completion state transferred to the worker.
pub(super) struct CommandDispatchPayload {
    /// Operation context used to reject work stopped before execution.
    pub(super) context: OperationContext,
    /// Runner policy selected for this request.
    pub(super) runner: CommandRunner,
    /// Command assembled from the MIME request.
    pub(super) command: Command,
    /// Staged input retained until the worker finishes.
    pub(super) input: ResourceRef<StagedMimeInput>,
    /// Shared command state that publishes the terminal outcome.
    pub(super) task: Arc<PreparedCommand>,
    /// Cancellation signal forwarded to the runner.
    pub(super) cancellation: CommandCancellation,
}
