// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! External operation state for a managed MIME command.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::task::Context;
use std::task::Poll;

use qubit_command::Command;
use qubit_command::CommandCancellation;
use qubit_command::CommandError;
use qubit_command::CommandErrorKind;
use qubit_command::CommandRunner;
use qubit_operation::ChildOutcomeKind;
use qubit_operation::ChildOutcomeSummary;
use qubit_operation::ChildRelease;
use qubit_operation::ChildSnapshot;
use qubit_operation::CleanupFailure;
use qubit_operation::DiagnosticSummary;
use qubit_operation::ExecutionFailure;
use qubit_operation::ExecutionPhase;
use qubit_operation::ExternalBindError;
use qubit_operation::OperationControl;
use qubit_operation::OperationId;
use qubit_operation::OperationOutcome;
use qubit_operation::OperationScope;
use qubit_operation::PanicSummary;
use qubit_operation::PreparedExternalOperation;
use qubit_operation::ResourceRef;
use qubit_operation::SettlementReport;
use qubit_operation::SettlementStatus;
use qubit_operation::StopReason;
use qubit_operation::StopReport;

use super::PollSignal;
use crate::command_execution::MimeCommandOutput;
use crate::command_execution::MimeExecutionWorker;
use crate::command_execution::SystemMimeCommandExecutor;
use crate::managed_temp_staging::StagedMimeInput;

/// Operation-owned state that runs one command and publishes its lifecycle
/// facts.
pub(in crate::command_execution) struct PreparedCommand {
    /// Fixed-capacity worker that executes the blocking command.
    worker: MimeExecutionWorker,
    /// Operation identity installed exactly once during bind.
    id: Mutex<Option<OperationId>>,
    /// Command and staged input retained until activation or abandonment.
    request: Mutex<Option<(CommandRunner, Command, ResourceRef<StagedMimeInput>)>>,
    /// Typed command outcome consumed once by the adapter future.
    outcome: PollSignal<OperationOutcome<MimeCommandOutput, CommandError>>,
    /// Compact outcome observed by the operation manager.
    summary: PollSignal<ChildOutcomeSummary>,
    /// Evidence that worker use of staged input has ended.
    release: PollSignal<ChildRelease>,
    /// Framework settlement report for the child operation.
    settlement: PollSignal<Arc<SettlementReport>>,
    /// Whether the worker has begun executing the command.
    started: AtomicBool,
    /// Whether the worker has relinquished staged input.
    released: AtomicBool,
    /// First requested stop reason, when one has been supplied.
    stop_reason: Mutex<Option<StopReason>>,
    /// Cancellation signal observed by the command runner.
    cancellation: CommandCancellation,
}

impl PreparedExternalOperation<MimeCommandOutput, CommandError> for PreparedCommand {
    /// Installs the operation identity before activation.
    ///
    /// # Parameters
    /// - `id`: Identity assigned by operation submission.
    ///
    /// # Returns
    /// `Ok(())` when this state is bound for the first time.
    ///
    /// # Errors
    /// Returns `AlreadyBound` if another operation has already claimed it.
    fn bind(&self, id: OperationId) -> Result<(), ExternalBindError> {
        let mut slot = self.id.lock().unwrap_or_else(|e| e.into_inner());
        if slot.is_some() {
            return Err(ExternalBindError::AlreadyBound);
        }
        *slot = Some(id);
        Ok(())
    }

    /// Dispatches the command to the fixed worker after submission accepts it.
    ///
    /// # Parameters
    /// - `_scope`: Operation scope supplied by the external adapter.
    ///
    /// # Returns
    /// `Ok(())` after dispatch, or an execution failure if the worker rejects
    /// it.
    ///
    /// # Errors
    /// Returns an execution failure when the fixed worker cannot accept the
    /// command job.
    fn activate(self: Arc<Self>, _scope: OperationScope) -> Result<(), ExecutionFailure> {
        if let Some(reason) = *self.stop_reason.lock().unwrap_or_else(|e| e.into_inner()) {
            self.request.lock().unwrap_or_else(|e| e.into_inner()).take();
            self.publish(OperationOutcome::Stopped(StopReport {
                reason,
                accepted_at: None,
            }));
            self.released.store(true, Ordering::Release);
            self.publish_release();
            return Ok(());
        }
        let (runner, command, input) = self
            .request
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
            .expect("prepared command can only activate once");
        let task = Arc::clone(&self);
        let cancellation = self.cancellation.clone();
        let job = Box::new(move || {
            task.started.store(true, Ordering::Release);
            if let Some(reason) = *task.stop_reason.lock().unwrap_or_else(|e| e.into_inner()) {
                drop(input);
                task.publish(OperationOutcome::Stopped(StopReport {
                    reason,
                    accepted_at: None,
                }));
                task.released.store(true, Ordering::Release);
                task.publish_release();
                return;
            }
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                SystemMimeCommandExecutor.run_cancellable(&runner, command, cancellation)
            }));
            drop(input); // only the worker's real return confirms release
            let outcome = match result {
                Ok(Ok(output)) => OperationOutcome::Succeeded(output),
                Ok(Err(error))
                    if matches!(
                        error.kind(),
                        CommandErrorKind::Cancelled | CommandErrorKind::CancelledBeforeStart
                    ) && error.cleanup_failures().is_empty() =>
                {
                    let reason = task
                        .stop_reason
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .as_ref()
                        .copied()
                        .unwrap_or(StopReason::UserCancelled);
                    OperationOutcome::Stopped(StopReport {
                        reason,
                        accepted_at: None,
                    })
                }
                Ok(Err(error)) => OperationOutcome::Failed(error),
                Err(_) => OperationOutcome::Panicked(PanicSummary {
                    operation_id: task
                        .id
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .expect("bound before panic"),
                    phase: ExecutionPhase::Running,
                    summary: DiagnosticSummary::new("MIME command worker panicked"),
                }),
            };
            task.publish(outcome);
            task.released.store(true, Ordering::Release);
            task.publish_release();
        });
        if let Err(job) = self.worker.try_dispatch(job) {
            drop(job);
            self.released.store(true, Ordering::Release);
            self.publish_release();
            return Err(ExecutionFailure {
                operation_id: self
                    .id
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .expect("bound before activation"),
                summary: DiagnosticSummary::new("MIME execution worker rejected command"),
                source: None,
            });
        }
        Ok(())
    }

    /// Releases an unactivated command request after adapter abandonment.
    fn abandon(self: Arc<Self>) {
        self.request.lock().unwrap_or_else(|e| e.into_inner()).take();
        self.released.store(true, Ordering::Release);
        self.publish_release();
    }

    /// Polls once for the typed command result.
    ///
    /// # Parameters
    /// - `cx`: Poll context used to register the waiting task.
    ///
    /// # Returns
    /// The command outcome when ready, or `Pending` with a registered waker.
    fn poll_take_outcome(&self, cx: &mut Context<'_>) -> Poll<OperationOutcome<MimeCommandOutput, CommandError>> {
        self.outcome.poll_take(cx)
    }
}

impl OperationControl for PreparedCommand {
    /// Requests command cancellation and records its stop reason.
    ///
    /// # Parameters
    /// - `reason`: Reason reported if the command resolves as stopped.
    fn request_stop(&self, reason: StopReason) {
        self.cancellation.cancel();
        *self.stop_reason.lock().unwrap_or_else(|e| e.into_inner()) = Some(reason);
    }

    /// Returns an observer snapshot of execution, release, and outcome state.
    fn snapshot(&self) -> ChildSnapshot {
        ChildSnapshot {
            operation_id: self
                .id
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .expect("bound before snapshot"),
            execution: if self.released.load(Ordering::Acquire) {
                ExecutionPhase::Finished
            } else if self.started.load(Ordering::Acquire) {
                ExecutionPhase::Running
            } else {
                ExecutionPhase::Queued
            },
            stop: self
                .stop_reason
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone()
                .map(|reason| StopReport {
                    reason,
                    accepted_at: None,
                }),
            outcome: self.summary.value(),
            input_released: self.released.load(Ordering::Acquire),
            settlement: self.settlement.value(),
        }
    }

    /// Polls the compact child outcome summary.
    ///
    /// # Parameters
    /// - `cx`: Poll context used to register the waiting task.
    ///
    /// # Returns
    /// A summary clone when ready, or `Pending` with a registered waker.
    fn poll_outcome(&self, cx: &mut Context<'_>) -> Poll<ChildOutcomeSummary> {
        self.summary.poll_clone(cx)
    }

    /// Polls for evidence that command input use has ended.
    ///
    /// # Parameters
    /// - `cx`: Poll context used to register the waiting task.
    ///
    /// # Returns
    /// Release evidence when ready, or `Pending` with a registered waker.
    fn poll_release(&self, cx: &mut Context<'_>) -> Poll<ChildRelease> {
        self.release.poll_clone(cx)
    }

    /// Polls for the child settlement report.
    ///
    /// # Parameters
    /// - `cx`: Poll context used to register the waiting task.
    ///
    /// # Returns
    /// A shared report when ready, or `Pending` with a registered waker.
    fn poll_settlement(&self, cx: &mut Context<'_>) -> Poll<Arc<SettlementReport>> {
        self.settlement.poll_clone(cx)
    }
}

impl PreparedCommand {
    /// Creates prepared child state before external submission is attempted.
    ///
    /// # Parameters
    /// - `worker`: Fixed-capacity worker that owns blocking execution.
    /// - `runner`: Command runner policy.
    /// - `command`: Structured command to execute.
    /// - `input`: Staged input retained through real worker completion.
    ///
    /// # Returns
    /// State ready to bind to one operation identity.
    pub(crate) fn new(
        worker: MimeExecutionWorker,
        runner: CommandRunner,
        command: Command,
        input: ResourceRef<StagedMimeInput>,
    ) -> Self {
        Self {
            worker,
            id: Mutex::new(None),
            request: Mutex::new(Some((runner, command, input))),
            outcome: PollSignal::new(),
            summary: PollSignal::new(),
            release: PollSignal::new(),
            settlement: PollSignal::new(),
            started: AtomicBool::new(false),
            released: AtomicBool::new(false),
            stop_reason: Mutex::new(None),
            cancellation: CommandCancellation::new(),
        }
    }

    /// Publishes one typed outcome and its compact observer summary.
    ///
    /// # Parameters
    /// - `outcome`: Completed operation outcome mapped by the framework.
    fn publish(&self, outcome: OperationOutcome<MimeCommandOutput, CommandError>) {
        let kind = match &outcome {
            OperationOutcome::Succeeded(_) => ChildOutcomeKind::Succeeded,
            OperationOutcome::Failed(_) => ChildOutcomeKind::Failed,
            OperationOutcome::Stopped(_) => ChildOutcomeKind::Stopped,
            OperationOutcome::Panicked(_) => ChildOutcomeKind::Panicked,
            OperationOutcome::ChildFailed(_) => ChildOutcomeKind::Failed,
            OperationOutcome::ExecutionFailed(_) => ChildOutcomeKind::ExecutionFailed,
        };
        let summary = ChildOutcomeSummary {
            operation_id: self
                .id
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .expect("bound before outcome"),
            kind,
            summary: DiagnosticSummary::new("MIME command worker completed"),
        };
        self.summary.publish(summary);
        self.outcome.publish(outcome);
    }

    /// Publishes confirmed release after the worker drops its input reference.
    fn publish_release(&self) {
        let operation_id = self
            .id
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .expect("bound before release");
        self.release.publish(ChildRelease {
            operation_id,
            confirmed_at: None,
        });
        self.settlement.publish(Arc::new(SettlementReport {
            operation_id,
            status: SettlementStatus::Released,
            released_count: 0,
            transferred_count: 0,
            recovery_count: 0,
            children: Arc::from([]),
            children_total: 0,
            children_truncated: false,
            cleanup_failures: Arc::<[CleanupFailure]>::from([]),
            cleanup_failures_total: 0,
            cleanup_failures_truncated: false,
            resource_reports: Arc::from([]),
            resource_reports_total: 0,
            resource_reports_truncated: false,
            transfers: Arc::from([]),
            transfers_total: 0,
            transfers_truncated: false,
            recoveries: Arc::from([]),
            recoveries_total: 0,
            recoveries_truncated: false,
            effects: Arc::from([]),
            effects_total: 0,
            effect_overflow: 0,
        }));
    }
}
