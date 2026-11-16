// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Asynchronous command boundary driven through operation's public adapter.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::task::{Context, Poll, Waker};

use qubit_command::{Command, CommandCancellation, CommandError, CommandErrorKind, CommandRunner};
use qubit_operation::{
    AsyncOperationHandle, ChildOutcomeKind, ChildOutcomeSummary, ChildRelease, ChildSnapshot,
    CleanupFailure, DiagnosticSummary, ExecutionFailure, ExecutionPhase, ExternalBindError,
    OperationControl, OperationId, OperationOptions, OperationOutcome, OperationScope,
    PanicSummary, PreparedExternalOperation, ResourceRef, SettlementReport, SettlementStatus,
    StopReason, StopReport,
};

use super::{
    ManagedMimeCommand, MimeCommandOutput, MimeExecutionWorker, SystemMimeCommandExecutor,
};
use crate::managed_temp_staging::StagedMimeInput;
use crate::{MimeError, MimeFuture};

/// Async command adapter. The command remains an operation child, so typed
/// outcome and worker input release are observed independently.
pub(crate) trait AsyncMimeCommandExecutor: Send + Sync {
    fn run_in<'a>(
        &'a self,
        scope: &'a OperationScope,
        command: ManagedMimeCommand,
        input: ResourceRef<StagedMimeInput>,
    ) -> MimeFuture<'a, MimeCommandOutput>;
}

struct PreparedCommand {
    worker: MimeExecutionWorker,
    id: Mutex<Option<OperationId>>,
    request: Mutex<Option<(CommandRunner, Command, ResourceRef<StagedMimeInput>)>>,
    outcome: Mutex<Option<OperationOutcome<MimeCommandOutput, CommandError>>>,
    summary_cache: Mutex<Option<ChildOutcomeSummary>>,
    outcome_waker: Mutex<Option<Waker>>,
    summary_waker: Mutex<Option<Waker>>,
    release_waker: Mutex<Option<Waker>>,
    settlement_waker: Mutex<Option<Waker>>,
    started: AtomicBool,
    released: AtomicBool,
    stop_reason: Mutex<Option<StopReason>>,
    cancellation: CommandCancellation,
}

impl PreparedExternalOperation<MimeCommandOutput, CommandError> for PreparedCommand {
    fn bind(&self, id: OperationId) -> Result<(), ExternalBindError> {
        let mut slot = self.id.lock().unwrap_or_else(|e| e.into_inner());
        if slot.is_some() {
            return Err(ExternalBindError::AlreadyBound);
        }
        *slot = Some(id);
        Ok(())
    }

    fn activate(self: Arc<Self>, _scope: OperationScope) -> Result<(), ExecutionFailure> {
        if let Some(reason) = *self.stop_reason.lock().unwrap_or_else(|e| e.into_inner()) {
            self.request
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take();
            self.publish(OperationOutcome::Stopped(StopReport {
                reason,
                accepted_at: None,
            }));
            self.released.store(true, Ordering::Release);
            wake(&self.outcome_waker);
            wake(&self.summary_waker);
            wake(&self.release_waker);
            wake(&self.settlement_waker);
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
                wake(&task.outcome_waker);
                wake(&task.summary_waker);
                wake(&task.release_waker);
                wake(&task.settlement_waker);
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
            wake(&task.outcome_waker);
            wake(&task.summary_waker);
            wake(&task.release_waker);
            wake(&task.settlement_waker);
        });
        if let Err(job) = self.worker.try_dispatch(job) {
            drop(job);
            self.released.store(true, Ordering::Release);
            wake(&self.release_waker);
            wake(&self.settlement_waker);
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

    fn abandon(self: Arc<Self>) {
        self.request
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        self.released.store(true, Ordering::Release);
        wake(&self.release_waker);
        wake(&self.settlement_waker);
    }

    fn poll_take_outcome(
        &self,
        cx: &mut Context<'_>,
    ) -> Poll<OperationOutcome<MimeCommandOutput, CommandError>> {
        if let Some(outcome) = self
            .outcome
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            return Poll::Ready(outcome);
        }
        *self.outcome_waker.lock().unwrap_or_else(|e| e.into_inner()) = Some(cx.waker().clone());
        Poll::Pending
    }
}

impl OperationControl for PreparedCommand {
    fn request_stop(&self, reason: StopReason) {
        self.cancellation.cancel();
        *self.stop_reason.lock().unwrap_or_else(|e| e.into_inner()) = Some(reason);
    }

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
            outcome: self.summary(),
            input_released: self.released.load(Ordering::Acquire),
            settlement: self.settlement(),
        }
    }

    fn poll_outcome(&self, cx: &mut Context<'_>) -> Poll<ChildOutcomeSummary> {
        if let Some(summary) = self.summary() {
            return Poll::Ready(summary);
        }
        *self.summary_waker.lock().unwrap_or_else(|e| e.into_inner()) = Some(cx.waker().clone());
        Poll::Pending
    }

    fn poll_release(&self, cx: &mut Context<'_>) -> Poll<ChildRelease> {
        if self.released.load(Ordering::Acquire) {
            return Poll::Ready(ChildRelease {
                operation_id: self
                    .id
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .expect("bound before release"),
                confirmed_at: None,
            });
        }
        *self.release_waker.lock().unwrap_or_else(|e| e.into_inner()) = Some(cx.waker().clone());
        Poll::Pending
    }

    fn poll_settlement(&self, cx: &mut Context<'_>) -> Poll<Arc<SettlementReport>> {
        if let Some(report) = self.settlement() {
            return Poll::Ready(report);
        }
        *self
            .settlement_waker
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(cx.waker().clone());
        Poll::Pending
    }
}

impl PreparedCommand {
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
        *self.summary_cache.lock().unwrap_or_else(|e| e.into_inner()) = Some(summary);
        *self.outcome.lock().unwrap_or_else(|e| e.into_inner()) = Some(outcome);
    }

    fn summary(&self) -> Option<ChildOutcomeSummary> {
        self.summary_cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    fn settlement(&self) -> Option<Arc<SettlementReport>> {
        if !self.released.load(Ordering::Acquire) {
            return None;
        }
        Some(Arc::new(SettlementReport {
            operation_id: self
                .id
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .expect("bound before settlement"),
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
        }))
    }
}

/// Manager-backed command adapter using the caller's parent scope and MIME
/// runtime group. Rejected submission retains the prepared operation until
/// the typed admission error is mapped.
pub(crate) struct ManagedCommandExecutor {
    worker: MimeExecutionWorker,
}
impl ManagedCommandExecutor {
    pub(crate) fn new(worker: MimeExecutionWorker) -> Self {
        Self { worker }
    }
}

impl AsyncMimeCommandExecutor for ManagedCommandExecutor {
    fn run_in<'a>(
        &'a self,
        scope: &'a OperationScope,
        request: ManagedMimeCommand,
        input: ResourceRef<StagedMimeInput>,
    ) -> MimeFuture<'a, MimeCommandOutput> {
        Box::pin(async move {
            let prepared = Arc::new(PreparedCommand {
                worker: self.worker.clone(),
                id: Mutex::new(None),
                request: Mutex::new(Some((request.runner, request.command, input))),
                outcome: Mutex::new(None),
                summary_cache: Mutex::new(None),
                outcome_waker: Mutex::new(None),
                summary_waker: Mutex::new(None),
                release_waker: Mutex::new(None),
                settlement_waker: Mutex::new(None),
                started: AtomicBool::new(false),
                released: AtomicBool::new(false),
                stop_reason: Mutex::new(None),
                cancellation: CommandCancellation::new(),
            });
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
                    MimeError::OperationAdmission(reason)
                })?;
            match handle
                .wait_outcome()
                .await
                .map_err(|_| MimeError::ExecutionServicesRequired)?
            {
                OperationOutcome::Succeeded(output) => Ok(output),
                OperationOutcome::Failed(error) => Err(MimeError::from(error)),
                OperationOutcome::Stopped(report) => Err(MimeError::OperationStopped(report)),
                OperationOutcome::Panicked(summary) => Err(MimeError::OperationPanicked(summary)),
                OperationOutcome::ExecutionFailed(failure) => {
                    Err(MimeError::OperationExecutionFailed(failure))
                }
                OperationOutcome::ChildFailed(failure) => {
                    Err(MimeError::ChildOperationFailed(failure))
                }
            }
        })
    }
}

fn wake(slot: &Mutex<Option<Waker>>) {
    if let Some(waker) = slot.lock().unwrap_or_else(|e| e.into_inner()).take() {
        waker.wake();
    }
}
