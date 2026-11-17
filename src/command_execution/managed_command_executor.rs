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
                .try_submit_external(OperationOptions::default(), Arc::clone(&prepared))
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
                    Err(MimeError::OperationExecutionFailed(Box::new(failure)))
                }
                OperationOutcome::ChildFailed(failure) => Err(MimeError::ChildOperationFailed(Box::new(failure))),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::sync::Arc;
    use std::task::Context;
    use std::task::Poll;
    use std::task::Wake;
    use std::task::Waker;
    use std::time::Duration;

    use qubit_command::Command;
    use qubit_command::CommandRunner;
    use qubit_operation::GroupOptions;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOutcome;

    use super::ManagedCommandExecutor;
    use crate::InputExtent;
    use crate::MimeError;
    use crate::OwnedMimeInput;
    use crate::command_execution::AsyncMimeCommandExecutor;
    use crate::command_execution::FixedMimeExecutionPort;
    use crate::command_execution::ManagedMimeCommand;
    use crate::command_execution::MimeCommandOutput;
    use crate::command_execution::MimeExecutionWorker;
    use crate::detector::MimeExecutionServices;
    use crate::managed_temp_staging::ManagedStagingLimits;
    use crate::managed_temp_staging::stage_owned_in;

    struct ThreadWake(std::thread::Thread);
    impl Wake for ThreadWake {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }

    fn block_on<F: Future>(future: F) -> F::Output {
        let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
        let mut context = Context::from_waker(&waker);
        let mut future = std::pin::pin!(future);
        loop {
            match future.as_mut().poll(&mut context) {
                Poll::Ready(value) => return value,
                Poll::Pending => std::thread::park(),
            }
        }
    }

    fn run_managed_command(
        command: Command,
        group_capacity: usize,
        foreign_runtime_group: bool,
        stop_during_child: bool,
    ) -> OperationOutcome<MimeCommandOutput, MimeError> {
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager
            .group(GroupOptions {
                capacity: group_capacity,
            })
            .unwrap();
        let port = Arc::new(FixedMimeExecutionPort::new(1, 4));
        let services = Arc::new(MimeExecutionServices::new(manager, group.clone(), port.clone(), 128).unwrap());
        services.prepare().unwrap();
        let limits = ManagedStagingLimits {
            max_bytes: 64,
            budget: services.staging_budget(),
            worker: MimeExecutionWorker::new(port.clone()),
        };
        let read_tree = services.new_read_tree(64);
        let executor = ManagedCommandExecutor::new(MimeExecutionWorker::new(port));
        let (mapped_stop_tx, mapped_stop_rx) = std::sync::mpsc::channel::<Option<qubit_operation::StopReason>>();
        let runtime_group = if foreign_runtime_group {
            let foreign_manager = OperationManager::new(Default::default()).unwrap();
            foreign_manager.group(GroupOptions { capacity: 1 }).unwrap()
        } else {
            group.clone()
        };
        let mut operation = block_on(group.try_submit_async(Default::default(), move |scope| async move {
            let input = OwnedMimeInput::bytes(Arc::from(b"managed command input".as_slice()), InputExtent::Complete);
            let staged = stage_owned_in(&scope, input, limits, read_tree).await?;
            let request = ManagedMimeCommand::new(
                CommandRunner::new(Duration::from_secs(10)).disable_logging(true),
                command,
                runtime_group,
            );
            let command_future = executor.run_in(&scope, request, staged);
            let result = if stop_during_child {
                let mut command_future = Box::pin(command_future);
                let mut stop_requested = false;
                std::future::poll_fn(|cx| {
                    let polled = command_future.as_mut().poll(cx);
                    if !stop_requested && polled.is_pending() {
                        let status = scope.request_stop(qubit_operation::StopReason::UserCancelled);
                        assert_eq!(qubit_operation::StopRequestStatus::Accepted, status);
                        stop_requested = true;
                    }
                    polled
                })
                .await
            } else {
                command_future.await
            };
            if stop_during_child {
                let reason = match &result {
                    Err(MimeError::OperationStopped(report)) => Some(report.reason),
                    _ => None,
                };
                mapped_stop_tx.send(reason).unwrap();
            }
            result
        }))
        .unwrap();
        let outcome = block_on(operation.wait_outcome()).unwrap();
        assert!(block_on(operation.wait_settlement()).all_released());
        if stop_during_child {
            assert_eq!(
                Some(qubit_operation::StopReason::ParentStopped),
                mapped_stop_rx.recv().unwrap(),
                "parent cancellation reaches the managed adapter as a child stop report"
            );
        }
        let shutdown = services.shutdown();
        block_on(shutdown.wait_services_settled()).unwrap();
        shutdown.stop_services().unwrap();
        outcome
    }

    #[test]
    fn test_managed_executor_maps_command_success_and_failure() {
        let success = run_managed_command(Command::new("rustc").arg("--version"), 4, false, false);
        let output = match success {
            OperationOutcome::Succeeded(output) => output,
            _ => panic!("managed command should return successful output"),
        };
        assert_eq!(Some(0), output.exit_code);
        assert!(output.stdout.starts_with(b"rustc "));

        let failure = run_managed_command(Command::new("__qubit_mime_managed_executor_missing__"), 4, false, false);
        assert!(
            matches!(failure, OperationOutcome::ChildFailed(_)),
            "the parent operation reports the failed managed command as a child failure"
        );
    }

    #[test]
    fn test_managed_executor_preserves_clean_nonzero_exit_output() {
        let outcome = run_managed_command(
            Command::new("rustc").arg("--qubit-mime-invalid-option"),
            4,
            false,
            false,
        );
        let output = match outcome {
            OperationOutcome::Succeeded(output) => output,
            _ => panic!("clean unexpected exits should preserve command output"),
        };

        assert_ne!(Some(0), output.exit_code);
        assert!(output.stdout_complete);
    }

    #[test]
    fn test_managed_executor_stops_running_child_and_settles_resources() {
        let outcome = run_managed_command(Command::new("sleep").arg("10"), 4, false, true);
        let stopped = matches!(outcome, OperationOutcome::ChildFailed(_));
        assert!(
            stopped,
            "a parent stop should surface the cancelled required child: {:?}",
            std::mem::discriminant(&outcome)
        );
    }

    #[test]
    fn test_managed_executor_maps_child_admission_rejection() {
        let outcome = run_managed_command(Command::new("must-not-start"), 1, false, false);
        assert!(matches!(
            outcome,
            OperationOutcome::Failed(MimeError::OperationAdmission(_))
        ));
    }

    #[test]
    fn test_managed_executor_rejects_group_from_another_manager() {
        let outcome = run_managed_command(Command::new("must-not-start"), 4, true, false);
        assert!(matches!(outcome, OperationOutcome::Failed(MimeError::GroupBinding(_))));
    }
}
