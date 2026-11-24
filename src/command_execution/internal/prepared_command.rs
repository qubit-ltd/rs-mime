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
use crate::detector::MimeExecutionDispatchFailureKind;
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
        if let Err(failure) = self.worker.try_dispatch_with_status(job) {
            let kind = failure.kind();
            drop(failure.into_job());
            self.released.store(true, Ordering::Release);
            self.publish_release();
            return Err(ExecutionFailure {
                operation_id: self
                    .id
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .expect("bound before activation"),
                summary: DiagnosticSummary::new(match kind {
                    MimeExecutionDispatchFailureKind::QueueFull => "MIME command worker queue is full",
                    MimeExecutionDispatchFailureKind::Unavailable => "MIME execution worker is unavailable",
                }),
                source: Some(Arc::new(kind)),
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
        let mut stop_reason = self.stop_reason.lock().unwrap_or_else(|e| e.into_inner());
        if stop_reason.is_none() {
            *stop_reason = Some(reason);
        }
        drop(stop_reason);
        self.cancellation.cancel();
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
            stop: (*self.stop_reason.lock().unwrap_or_else(|e| e.into_inner())).map(|reason| StopReport {
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
    #[must_use]
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
        let (kind, summary_text) = match &outcome {
            OperationOutcome::Succeeded(_) => (ChildOutcomeKind::Succeeded, "child succeeded"),
            OperationOutcome::Failed(_) => (ChildOutcomeKind::Failed, "child failed"),
            OperationOutcome::Stopped(_) => (ChildOutcomeKind::Stopped, "child stopped"),
            OperationOutcome::Panicked(panic) => (ChildOutcomeKind::Panicked, panic.summary.as_str()),
            OperationOutcome::ChildFailed(_) => (ChildOutcomeKind::Failed, "child failed"),
            OperationOutcome::ExecutionFailed(failure) => (ChildOutcomeKind::ExecutionFailed, failure.summary.as_str()),
        };
        let summary = ChildOutcomeSummary {
            operation_id: self
                .id
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .expect("bound before outcome"),
            kind,
            summary: DiagnosticSummary::new(summary_text),
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
            external_settlement: None,
        }));
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

    use qubit_command::Command;
    use qubit_command::CommandError;
    use qubit_command::CommandErrorKind;
    use qubit_command::CommandRunner;
    use qubit_operation::ChildOutcomeKind;
    use qubit_operation::ChildSnapshot;
    use qubit_operation::ExecutionFailure;
    use qubit_operation::OperationControl;
    use qubit_operation::OperationOutcome;
    use qubit_operation::PreparedExternalOperation;
    use qubit_operation::StopReason;

    use super::PreparedCommand;
    use crate::InputExtent;
    use crate::MimeError;
    use crate::OwnedMimeInput;
    use crate::command_execution::FixedMimeExecutionPort;
    use crate::command_execution::MimeCommandOutput;
    use crate::command_execution::MimeExecutionWorker;
    use crate::detector::MimeExecutionPort;
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

    #[derive(Clone, Copy)]
    enum PreparedCommandLock {
        Identity,
        Request,
        StopReason,
    }

    fn poison_prepared_command_lock(command: Arc<PreparedCommand>, lock: PreparedCommandLock) {
        let poisoned = std::thread::spawn(move || match lock {
            PreparedCommandLock::Identity => {
                let _guard = command.id.lock().unwrap();
                panic!("poison prepared identity lock");
            }
            PreparedCommandLock::Request => {
                let _guard = command.request.lock().unwrap();
                panic!("poison prepared request lock");
            }
            PreparedCommandLock::StopReason => {
                let _guard = command.stop_reason.lock().unwrap();
                panic!("poison prepared stop-reason lock");
            }
        })
        .join();
        assert!(poisoned.is_err(), "the selected lock must be poisoned");
    }

    fn execute_prepared_command(
        command: Command,
        reject_dispatch: bool,
        observe_running: bool,
        stop_when_running: bool,
        timeout: std::time::Duration,
        strict_output_limit: bool,
        poison_lock: Option<PreparedCommandLock>,
    ) -> (
        Result<OperationOutcome<MimeCommandOutput, CommandError>, ExecutionFailure>,
        ChildSnapshot,
    ) {
        let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
        let group = manager.group(qubit_operation::GroupOptions { capacity: 4 }).unwrap();
        let port = Arc::new(FixedMimeExecutionPort::new(1, 4));
        let services = Arc::new(MimeExecutionServices::new(manager, group.clone(), port.clone(), 1024).unwrap());
        services.prepare().unwrap();
        let limits = ManagedStagingLimits {
            max_bytes: 64,
            budget: services.staging_budget(),
            worker: MimeExecutionWorker::new(port),
        };
        let read_tree = services.new_read_tree(64);
        let body_services = Arc::clone(&services);
        let input = OwnedMimeInput::bytes(Arc::from(b"prepared-command-input".as_slice()), InputExtent::Complete);
        let mut operation = block_on(group.try_submit_async(Default::default(), move |scope| async move {
            let staged = stage_owned_in(&scope, input, limits, read_tree).await?;
            let command_worker = Arc::new(FixedMimeExecutionPort::new(1, if reject_dispatch { 1 } else { 4 }));
            command_worker.prepare()?;
            let (release_blocker, queued_job_done) = if reject_dispatch {
                let (started_tx, started_rx) = std::sync::mpsc::channel();
                let (release_tx, release_rx) = std::sync::mpsc::channel();
                assert!(
                    command_worker
                        .try_dispatch(Box::new(move || {
                            started_tx.send(()).unwrap();
                            release_rx.recv().unwrap();
                        }))
                        .is_ok()
                );
                started_rx.recv().unwrap();
                let (done_tx, done_rx) = std::sync::mpsc::channel();
                assert!(
                    command_worker
                        .try_dispatch(Box::new(move || done_tx.send(()).unwrap()))
                        .is_ok()
                );
                (Some(release_tx), Some(done_rx))
            } else {
                (None, None)
            };
            let runner = CommandRunner::new(timeout).disable_logging(true);
            let runner = if strict_output_limit {
                runner.bounded_output(1)
            } else {
                runner
            };
            let prepared = Arc::new(PreparedCommand::new(
                MimeExecutionWorker::new(command_worker.clone()),
                runner,
                command,
                staged,
            ));
            if let Some(lock) = poison_lock {
                poison_prepared_command_lock(Arc::clone(&prepared), lock);
            }
            let operation_id = scope.id();
            prepared.bind(operation_id).unwrap();
            let activation = Arc::clone(&prepared).activate(scope);
            let result = match activation {
                Ok(()) => {
                    if observe_running {
                        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
                        loop {
                            let snapshot = prepared.snapshot();
                            if snapshot.execution == qubit_operation::ExecutionPhase::Running {
                                assert!(!snapshot.input_released);
                                {
                                    let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
                                    let mut context = Context::from_waker(&waker);
                                    assert!(matches!(prepared.poll_take_outcome(&mut context), Poll::Pending));
                                    assert!(matches!(prepared.poll_outcome(&mut context), Poll::Pending));
                                    assert!(matches!(prepared.poll_release(&mut context), Poll::Pending));
                                    assert!(matches!(prepared.poll_settlement(&mut context), Poll::Pending));
                                }
                                if stop_when_running {
                                    prepared.request_stop(StopReason::Timeout);
                                }
                                break;
                            }
                            assert!(std::time::Instant::now() < deadline, "worker never reported running");
                            std::thread::yield_now();
                        }
                    }
                    let mut outcome = None;
                    let mut release = None;
                    std::future::poll_fn(|cx| {
                        if outcome.is_none()
                            && let std::task::Poll::Ready(value) = prepared.poll_take_outcome(cx)
                        {
                            outcome = Some(value);
                        }
                        if release.is_none()
                            && let std::task::Poll::Ready(value) = prepared.poll_release(cx)
                        {
                            release = Some(value);
                        }
                        if outcome.is_some() && release.is_some() {
                            std::task::Poll::Ready(())
                        } else {
                            std::task::Poll::Pending
                        }
                    })
                    .await;
                    assert_eq!(
                        operation_id,
                        release.expect("release signal must be observed").operation_id
                    );
                    Ok(outcome.expect("outcome signal must be observed"))
                }
                Err(error) => Err(error),
            };
            let settlement = std::future::poll_fn(|cx| prepared.poll_settlement(cx)).await;
            assert_eq!(qubit_operation::SettlementStatus::Released, settlement.status);
            let snapshot = prepared.snapshot();
            if let Some(release) = release_blocker {
                release.send(()).unwrap();
                queued_job_done.expect("queued callback").recv().unwrap();
            }
            command_worker.shutdown()?;
            Ok::<_, MimeError>((result, snapshot))
        }))
        .unwrap();
        let result = match block_on(operation.wait_outcome()).unwrap() {
            OperationOutcome::Succeeded(value) => value,
            _ => panic!("test operation did not return its command lifecycle facts"),
        };
        assert!(block_on(operation.wait_settlement()).all_released());
        let shutdown = body_services.shutdown();
        let _ = block_on(shutdown.wait_services_settled()).unwrap();
        let _ = shutdown.stop_services().unwrap();
        result
    }

    #[test]
    fn test_stop_before_activation_publishes_terminal_outcome_and_release() {
        let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
        let group = manager.group(qubit_operation::GroupOptions { capacity: 4 }).unwrap();
        let port = Arc::new(FixedMimeExecutionPort::new(1, 4));
        let services = Arc::new(MimeExecutionServices::new(manager, group.clone(), port.clone(), 1024).unwrap());
        services.prepare().unwrap();
        let limits = ManagedStagingLimits {
            max_bytes: 64,
            budget: services.staging_budget(),
            worker: MimeExecutionWorker::new(port.clone()),
        };
        let read_tree = services.new_read_tree(64);
        let body_services = Arc::clone(&services);
        let input = OwnedMimeInput::bytes(Arc::from(b"cancel before dispatch".as_slice()), InputExtent::Complete);

        let mut handle = block_on(group.try_submit_async(Default::default(), move |scope| async move {
            let abandoned_input = stage_owned_in(
                &scope,
                OwnedMimeInput::bytes(Arc::from(b"abandoned request".as_slice()), InputExtent::Complete),
                limits.clone(),
                Arc::clone(&read_tree),
            )
            .await?;
            let abandoned = Arc::new(PreparedCommand::new(
                MimeExecutionWorker::new(port.clone()),
                CommandRunner::new(std::time::Duration::from_secs(1)),
                Command::new("must-not-run"),
                abandoned_input,
            ));
            abandoned.bind(scope.id()).unwrap();
            let request_lock = Arc::clone(&abandoned);
            assert!(
                std::thread::spawn(move || {
                    let _guard = request_lock.request.lock().unwrap();
                    panic!("poison abandoned request lock");
                })
                .join()
                .is_err()
            );
            Arc::clone(&abandoned).abandon();
            let abandoned_snapshot = abandoned.snapshot();
            assert_eq!(scope.id(), abandoned_snapshot.operation_id);
            assert_eq!(qubit_operation::ExecutionPhase::Finished, abandoned_snapshot.execution);
            assert!(abandoned_snapshot.input_released);
            assert!(
                abandoned_snapshot.outcome.is_none(),
                "abandonment is not a command outcome"
            );
            assert!(abandoned_snapshot.settlement.is_some());
            {
                let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
                let mut context = Context::from_waker(&waker);
                assert!(matches!(abandoned.poll_take_outcome(&mut context), Poll::Pending));
                assert!(matches!(abandoned.poll_outcome(&mut context), Poll::Pending));
            }
            let abandoned_release = std::future::poll_fn(|cx| abandoned.poll_release(cx)).await;
            assert_eq!(scope.id(), abandoned_release.operation_id);
            let abandoned_settlement = std::future::poll_fn(|cx| abandoned.poll_settlement(cx)).await;
            assert_eq!(qubit_operation::SettlementStatus::Released, abandoned_settlement.status);
            drop(abandoned);

            let staged = stage_owned_in(&scope, input, limits, read_tree).await?;
            let command = Arc::new(PreparedCommand::new(
                MimeExecutionWorker::new(port),
                CommandRunner::new(std::time::Duration::from_secs(1)),
                Command::new("must-not-run"),
                staged,
            ));
            command.bind(scope.id()).unwrap();
            assert!(
                command.bind(scope.id()).is_err(),
                "a command binds to one operation only"
            );
            let queued_snapshot = command.snapshot();
            assert_eq!(scope.id(), queued_snapshot.operation_id);
            assert_eq!(qubit_operation::ExecutionPhase::Queued, queued_snapshot.execution);
            assert!(queued_snapshot.stop.is_none());

            command.request_stop(StopReason::Timeout);
            command.request_stop(StopReason::ParentStopped);
            assert!(matches!(command.snapshot().stop, Some(report) if report.reason == StopReason::Timeout));
            Arc::clone(&command).activate(scope).unwrap();
            let outcome = std::future::poll_fn(|cx| command.poll_take_outcome(cx)).await;
            assert!(matches!(outcome, OperationOutcome::Stopped(report) if report.reason == StopReason::Timeout));
            let snapshot = command.snapshot();
            assert_eq!(qubit_operation::ExecutionPhase::Finished, snapshot.execution);
            assert!(snapshot.input_released);
            assert!(matches!(snapshot.stop, Some(report) if report.reason == StopReason::Timeout));
            assert_eq!(
                Some(ChildOutcomeKind::Stopped),
                snapshot.outcome.map(|summary| summary.kind)
            );
            assert!(snapshot.settlement.is_some());
            Ok::<(), MimeError>(())
        }))
        .unwrap();

        assert!(matches!(
            block_on(handle.wait_outcome()).unwrap(),
            OperationOutcome::Succeeded(())
        ));
        assert!(block_on(handle.wait_settlement()).all_released());
        let shutdown = body_services.shutdown();
        let _ = block_on(shutdown.wait_services_settled()).unwrap();
        let _ = shutdown.stop_services().unwrap();
    }

    #[test]
    fn test_prepared_command_recovers_poisoned_state_locks_before_stopping() {
        let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
        let group = manager.group(qubit_operation::GroupOptions { capacity: 1 }).unwrap();
        let port = Arc::new(FixedMimeExecutionPort::new(1, 1));
        let services = Arc::new(MimeExecutionServices::new(manager, group.clone(), port.clone(), 64).unwrap());
        services.prepare().unwrap();
        let worker = MimeExecutionWorker::new(port);
        let limits = ManagedStagingLimits {
            max_bytes: 32,
            budget: services.staging_budget(),
            worker: worker.clone(),
        };
        let body_services = Arc::clone(&services);
        let mut operation = block_on(group.try_submit_async(Default::default(), move |scope| async move {
            let staged = stage_owned_in(
                &scope,
                OwnedMimeInput::bytes(Arc::from(b"stopped input".as_slice()), InputExtent::Complete),
                limits,
                body_services.new_read_tree(32),
            )
            .await?;
            let command = Arc::new(PreparedCommand::new(
                worker,
                CommandRunner::new(std::time::Duration::from_secs(1)),
                Command::new("must-not-run"),
                staged,
            ));

            let id_lock = Arc::clone(&command);
            assert!(
                std::thread::spawn(move || {
                    let _guard = id_lock.id.lock().unwrap();
                    panic!("poison operation identity lock");
                })
                .join()
                .is_err()
            );
            let request_lock = Arc::clone(&command);
            assert!(
                std::thread::spawn(move || {
                    let _guard = request_lock.request.lock().unwrap();
                    panic!("poison prepared request lock");
                })
                .join()
                .is_err()
            );
            let stop_lock = Arc::clone(&command);
            assert!(
                std::thread::spawn(move || {
                    let _guard = stop_lock.stop_reason.lock().unwrap();
                    panic!("poison stop reason lock");
                })
                .join()
                .is_err()
            );

            command
                .bind(scope.id())
                .expect("binding should recover the poisoned identity lock");
            command.request_stop(StopReason::Timeout);
            Arc::clone(&command)
                .activate(scope)
                .expect("a stopped command should settle without dispatch");
            let outcome = std::future::poll_fn(|cx| command.poll_take_outcome(cx)).await;
            assert!(matches!(outcome, OperationOutcome::Stopped(report) if report.reason == StopReason::Timeout));
            assert!(command.snapshot().input_released);
            Ok::<_, MimeError>(())
        }))
        .unwrap();

        assert!(matches!(
            block_on(operation.wait_outcome()).unwrap(),
            OperationOutcome::Succeeded(())
        ));
        assert!(block_on(operation.wait_settlement()).all_released());
        let shutdown = services.shutdown();
        let _ = block_on(shutdown.wait_services_settled()).unwrap();
        let _ = shutdown.stop_services().unwrap();
    }

    #[test]
    fn test_activation_maps_success_and_confirms_input_release() {
        let (result, snapshot) = execute_prepared_command(
            Command::new("rustc").arg("--version"),
            false,
            false,
            false,
            std::time::Duration::from_secs(10),
            false,
            None,
        );
        let output = match result {
            Ok(OperationOutcome::Succeeded(output)) => output,
            _ => panic!("version command should complete successfully"),
        };
        assert_eq!(Some(0), output.exit_code);
        assert!(output.stdout.starts_with(b"rustc "));
        assert_eq!(qubit_operation::ExecutionPhase::Finished, snapshot.execution);
        assert!(snapshot.input_released);
        assert_eq!(
            Some(ChildOutcomeKind::Succeeded),
            snapshot.outcome.map(|summary| summary.kind)
        );
        assert!(snapshot.settlement.is_some());
    }

    #[test]
    fn test_activation_recovers_poisoned_request_lock_and_releases_input() {
        let (result, snapshot) = execute_prepared_command(
            Command::new("rustc").arg("--version"),
            false,
            false,
            false,
            std::time::Duration::from_secs(10),
            false,
            Some(PreparedCommandLock::Request),
        );

        assert!(matches!(result, Ok(OperationOutcome::Succeeded(_))));
        assert_eq!(qubit_operation::ExecutionPhase::Finished, snapshot.execution);
        assert!(snapshot.input_released);
        assert_eq!(
            Some(ChildOutcomeKind::Succeeded),
            snapshot.outcome.map(|summary| summary.kind)
        );
        assert!(snapshot.settlement.is_some());
    }

    #[test]
    fn test_activation_maps_spawn_failure_and_worker_rejection() {
        let (result, failed_snapshot) = execute_prepared_command(
            Command::new("__qubit_mime_prepared_command_missing_test_executable__"),
            false,
            false,
            false,
            std::time::Duration::from_secs(10),
            false,
            None,
        );
        assert!(matches!(result, Ok(OperationOutcome::Failed(error)) if error.kind() == CommandErrorKind::SpawnFailed));
        assert_eq!(qubit_operation::ExecutionPhase::Finished, failed_snapshot.execution);
        assert_eq!(
            Some(ChildOutcomeKind::Failed),
            failed_snapshot.outcome.map(|summary| summary.kind)
        );
        assert!(failed_snapshot.input_released);
        assert!(failed_snapshot.settlement.is_some());

        let (result, rejected_snapshot) = execute_prepared_command(
            Command::new("must-not-run"),
            true,
            false,
            false,
            std::time::Duration::from_secs(10),
            false,
            Some(PreparedCommandLock::Identity),
        );
        assert!(result.is_err(), "a full command queue rejects activation");
        assert_eq!(qubit_operation::ExecutionPhase::Finished, rejected_snapshot.execution);
        assert!(rejected_snapshot.input_released);
        assert!(
            rejected_snapshot.outcome.is_none(),
            "dispatch rejection is execution failure, not command outcome"
        );
        assert!(rejected_snapshot.settlement.is_some());
    }

    #[cfg(unix)]
    #[test]
    fn test_running_snapshot_retains_input_until_command_exit() {
        let (result, snapshot) = execute_prepared_command(
            Command::new("sleep").arg("0.2"),
            false,
            true,
            false,
            std::time::Duration::from_secs(10),
            false,
            None,
        );
        assert!(matches!(result, Ok(OperationOutcome::Succeeded(_))));
        assert_eq!(qubit_operation::ExecutionPhase::Finished, snapshot.execution);
        assert!(snapshot.input_released);
        assert_eq!(
            Some(ChildOutcomeKind::Succeeded),
            snapshot.outcome.map(|summary| summary.kind)
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_stop_during_running_command_cancels_child_and_releases_input() {
        let (result, snapshot) = execute_prepared_command(
            Command::new("sleep").arg("10"),
            false,
            true,
            true,
            std::time::Duration::from_secs(10),
            false,
            Some(PreparedCommandLock::StopReason),
        );

        assert!(matches!(result, Ok(OperationOutcome::Stopped(report)) if report.reason == StopReason::Timeout));
        assert_eq!(qubit_operation::ExecutionPhase::Finished, snapshot.execution);
        assert!(snapshot.input_released);
        assert_eq!(
            Some(ChildOutcomeKind::Stopped),
            snapshot.outcome.map(|summary| summary.kind)
        );
        assert!(snapshot.settlement.is_some());
    }

    #[cfg(unix)]
    #[test]
    fn test_command_timeout_reports_failure_and_releases_staged_input() {
        let (result, snapshot) = execute_prepared_command(
            Command::new("sleep").arg("10"),
            false,
            false,
            false,
            std::time::Duration::from_millis(100),
            false,
            None,
        );

        assert!(matches!(result, Ok(OperationOutcome::Failed(error)) if error.kind() == CommandErrorKind::TimedOut));
        assert_eq!(qubit_operation::ExecutionPhase::Finished, snapshot.execution);
        assert!(snapshot.input_released);
        assert_eq!(
            Some(ChildOutcomeKind::Failed),
            snapshot.outcome.map(|summary| summary.kind)
        );
        assert!(snapshot.settlement.is_some());
    }

    #[test]
    fn test_truncated_stdout_fails_command_and_releases_staged_input() {
        let (result, snapshot) = execute_prepared_command(
            Command::new("rustc").arg("--version"),
            false,
            false,
            false,
            std::time::Duration::from_secs(10),
            true,
            None,
        );

        assert!(
            matches!(result, Ok(OperationOutcome::Failed(error)) if error.kind() == CommandErrorKind::OutputTruncated)
        );
        assert_eq!(qubit_operation::ExecutionPhase::Finished, snapshot.execution);
        assert!(snapshot.input_released);
        assert_eq!(
            Some(ChildOutcomeKind::Failed),
            snapshot.outcome.map(|summary| summary.kind)
        );
        assert!(snapshot.settlement.is_some());
    }

    #[test]
    fn test_stdin_open_failure_publishes_failed_outcome_and_releases_input() {
        let (result, snapshot) = execute_prepared_command(
            Command::new("rustc")
                .arg("--version")
                .stdin_file("/qubit-mime-missing-command-input-test"),
            false,
            false,
            false,
            std::time::Duration::from_secs(10),
            false,
            None,
        );

        assert!(
            matches!(result, Ok(OperationOutcome::Failed(error)) if error.kind() == CommandErrorKind::OpenInputFailed)
        );
        assert_eq!(qubit_operation::ExecutionPhase::Finished, snapshot.execution);
        assert!(snapshot.input_released);
        assert_eq!(
            Some(ChildOutcomeKind::Failed),
            snapshot.outcome.map(|summary| summary.kind)
        );
        assert!(snapshot.settlement.is_some());
    }

    #[cfg(unix)]
    #[test]
    fn test_stop_after_dispatch_but_before_worker_start_releases_without_spawning() {
        let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
        let group = manager.group(qubit_operation::GroupOptions { capacity: 4 }).unwrap();
        let port = Arc::new(FixedMimeExecutionPort::new(1, 2));
        let services = Arc::new(MimeExecutionServices::new(manager, group.clone(), port.clone(), 1024).unwrap());
        services.prepare().unwrap();
        let limits = ManagedStagingLimits {
            max_bytes: 64,
            budget: services.staging_budget(),
            worker: MimeExecutionWorker::new(port.clone()),
        };
        let read_tree = services.new_read_tree(64);
        let body_services = Arc::clone(&services);
        let mut operation = block_on(group.try_submit_async(Default::default(), move |scope| async move {
            let input = OwnedMimeInput::bytes(
                Arc::from(b"queued cancellation input".as_slice()),
                InputExtent::Complete,
            );
            let staged = stage_owned_in(&scope, input, limits, read_tree).await?;
            let (started_tx, started_rx) = std::sync::mpsc::channel();
            let (release_tx, release_rx) = std::sync::mpsc::channel();
            assert!(
                MimeExecutionWorker::new(port.clone())
                    .try_dispatch(Box::new(move || {
                        started_tx.send(()).unwrap();
                        let _ = release_rx.recv();
                    }))
                    .is_ok()
            );
            started_rx.recv().unwrap();

            let command = Arc::new(PreparedCommand::new(
                MimeExecutionWorker::new(port),
                CommandRunner::new(std::time::Duration::from_secs(1)).disable_logging(true),
                Command::new("must-not-start-after-queued-stop"),
                staged,
            ));
            poison_prepared_command_lock(Arc::clone(&command), PreparedCommandLock::StopReason);
            command.bind(scope.id()).unwrap();
            let operation_id = scope.id();
            Arc::clone(&command).activate(scope).unwrap();
            command.request_stop(StopReason::ParentStopped);
            command.request_stop(StopReason::Timeout);
            release_tx.send(()).unwrap();

            let outcome = std::future::poll_fn(|cx| command.poll_take_outcome(cx)).await;
            assert!(matches!(outcome, OperationOutcome::Stopped(report) if report.reason == StopReason::ParentStopped));
            let summary = std::future::poll_fn(|cx| command.poll_outcome(cx)).await;
            assert_eq!(ChildOutcomeKind::Stopped, summary.kind);
            let release = std::future::poll_fn(|cx| command.poll_release(cx)).await;
            assert_eq!(operation_id, release.operation_id);
            let settlement = std::future::poll_fn(|cx| command.poll_settlement(cx)).await;
            assert_eq!(qubit_operation::SettlementStatus::Released, settlement.status);
            let snapshot = command.snapshot();
            assert_eq!(qubit_operation::ExecutionPhase::Finished, snapshot.execution);
            assert!(matches!(snapshot.stop, Some(report) if report.reason == StopReason::ParentStopped));
            assert!(snapshot.input_released);
            Ok::<_, MimeError>(())
        }))
        .unwrap();

        assert!(matches!(
            block_on(operation.wait_outcome()).unwrap(),
            OperationOutcome::Succeeded(())
        ));
        assert!(block_on(operation.wait_settlement()).all_released());
        let shutdown = body_services.shutdown();
        let _ = block_on(shutdown.wait_services_settled()).unwrap();
        let _ = shutdown.stop_services().unwrap();
    }
}
