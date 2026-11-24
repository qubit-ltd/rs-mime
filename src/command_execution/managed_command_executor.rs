// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Managed async command adapter implementation.

use std::sync::Arc;

use qubit_command::CommandError;
use qubit_operation::AsyncOperationHandle;
use qubit_operation::ChildRequirement;
use qubit_operation::OperationOptions;
use qubit_operation::OperationOutcome;
use qubit_operation::OperationScope;
use qubit_operation::ResourceRef;

use super::AsyncMimeCommandExecutor;
use super::ManagedMimeCommand;
use super::MimeCommandOutput;
use super::MimeExecutionWorker;
use super::internal::PreparedCommand;
use crate::MimeError;
use crate::MimeFuture;
use crate::detector::MimeExecutionDispatchFailureKind;
use crate::managed_temp_staging::StagedMimeInput;

/// Manager-backed command adapter using the caller's parent scope and MIME
/// runtime group. Rejected submission retains the prepared operation until
/// the typed admission error is mapped.
pub(crate) struct ManagedCommandExecutor {
    /// Worker used for blocking process execution.
    worker: MimeExecutionWorker,
}
impl ManagedCommandExecutor {
    /// Creates an executor bound to a prepared worker.
    ///
    /// # Parameters
    /// - `worker`: Fixed-capacity worker that accepts command jobs.
    ///
    /// # Returns
    /// An adapter that submits commands through operation's external API.
    #[must_use]
    pub(crate) fn new(worker: MimeExecutionWorker) -> Self {
        Self { worker }
    }
}

impl AsyncMimeCommandExecutor for ManagedCommandExecutor {
    /// Submits one command as an operation child and awaits its typed outcome.
    ///
    /// # Parameters
    /// - `scope`: Parent operation scope inherited by the command child.
    /// - `request`: Command runner, invocation, and runtime group.
    /// - `input`: Staged input held until the command worker returns.
    ///
    /// # Returns
    /// Complete command output or the mapped MIME execution error.
    ///
    /// # Errors
    /// Returns admission, cancellation, command, or settlement errors from the
    /// managed operation child.
    fn run_in<'a>(
        &'a self,
        scope: &'a OperationScope,
        request: ManagedMimeCommand,
        input: ResourceRef<StagedMimeInput>,
    ) -> MimeFuture<'a, MimeCommandOutput> {
        Box::pin(async move {
            let prepared = Arc::new(PreparedCommand::new(
                self.worker.clone(),
                request.runner,
                request.command,
                input,
            ));
            let manager = scope
                .manager()
                .with_group(&request.runtime_group)
                .map_err(MimeError::GroupBinding)?
                .as_async();
            let mut handle: AsyncOperationHandle<MimeCommandOutput, CommandError> = manager
                .try_submit_external_as_child(
                    OperationOptions {
                        requirement: ChildRequirement::Optional,
                        ..OperationOptions::default()
                    },
                    Arc::clone(&prepared),
                )
                .map_err(|error| {
                    let (reason, rejected) = error.into_parts();
                    drop(rejected); // exact prepared task remains intact through rejection mapping
                    MimeError::OperationAdmission(Box::new(reason))
                })?;
            match handle
                .wait_outcome()
                .await
                .map_err(|_| MimeError::ExecutionServicesRequired)?
            {
                OperationOutcome::Succeeded(output) => Ok(output),
                OperationOutcome::Failed(error) => Err(MimeError::from(error)),
                OperationOutcome::Stopped(report) => Err(MimeError::OperationStopped(Box::new(report))),
                OperationOutcome::Panicked(summary) => Err(MimeError::OperationPanicked(Box::new(summary))),
                OperationOutcome::ExecutionFailed(failure) => {
                    if failure.source.as_ref().is_some_and(|source| {
                        source.downcast_ref::<MimeExecutionDispatchFailureKind>()
                            == Some(&MimeExecutionDispatchFailureKind::QueueFull)
                    }) {
                        Err(MimeError::CommandQueueBusy)
                    } else {
                        Err(MimeError::OperationExecutionFailed(Box::new(failure)))
                    }
                }
                OperationOutcome::ChildFailed(failure) => Err(MimeError::ChildOperationFailed(Box::new(failure))),
            }
        })
    }
}
