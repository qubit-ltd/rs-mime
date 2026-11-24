// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

//! Tests the private command result boundary without shell or PATH mutation.
use std::time::Duration;

use qubit_command::Command;
use qubit_command::CommandErrorKind;
use qubit_command::CommandRunner;

use crate::command_execution::MimeCommandExecutor;
use crate::command_execution::MimeCommandOutput;
use crate::command_execution::SystemMimeCommandExecutor;
use crate::command_execution::require_complete_stdout;
use crate::detector::MimeExecutionDispatchFailureKind;
use crate::detector::MimeExecutionPort;

#[test]
fn test_legacy_execution_port_fallback_preserves_rejected_job_and_debug_reason() {
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::Ordering;

    #[derive(Debug)]
    struct LegacyRejectPort;

    impl MimeExecutionPort for LegacyRejectPort {
        fn prepare(&self) -> Result<(), crate::MimeError> {
            Ok(())
        }

        fn try_dispatch(
            &self,
            job: Box<dyn FnOnce() + Send + 'static>,
        ) -> Result<(), Box<dyn FnOnce() + Send + 'static>> {
            Err(job)
        }

        fn shutdown(&self) -> Result<(), crate::MimeError> {
            Ok(())
        }
    }

    let ran = Arc::new(AtomicBool::new(false));
    let job_ran = Arc::clone(&ran);
    let failure = LegacyRejectPort
        .try_dispatch_with_status(Box::new(move || job_ran.store(true, Ordering::SeqCst)))
        .expect_err("the legacy dispatcher deliberately rejects work");

    assert_eq!(MimeExecutionDispatchFailureKind::Unavailable, failure.kind());
    let debug = format!("{failure:?}");
    assert!(debug.contains("Unavailable"));
    assert!(!debug.contains("job:"));
    failure.into_job()();
    assert!(ran.load(Ordering::SeqCst), "the original job remains recoverable");
}

#[test]
fn test_require_complete_stdout_rejects_unusable_streams() {
    for (bytes, truncated, complete, expected) in [
        (b"video\n".as_slice(), true, true, "stdout was truncated"),
        (b"video\n".as_slice(), false, false, "stdout is incomplete"),
        ([255].as_slice(), false, true, "stdout is not valid UTF-8"),
    ] {
        let output = MimeCommandOutput {
            exit_code: Some(0),
            stdout: bytes.to_vec(),
            stdout_truncated: truncated,
            stdout_complete: complete,
        };
        assert_eq!(require_complete_stdout(&output), Err(expected));
    }
    let output = MimeCommandOutput {
        exit_code: Some(0),
        stdout: b"video\n".to_vec(),
        stdout_truncated: false,
        stdout_complete: true,
    };
    assert_eq!(require_complete_stdout(&output), Ok("video\n"));
}

#[test]
fn test_executor_preserves_actual_status_and_strict_failures() {
    let runner = CommandRunner::new(Duration::from_secs(10)).disable_logging(true);
    let output = SystemMimeCommandExecutor
        .run(
            &runner.clone().success_exit_codes(&[7]),
            Command::new("rustc").arg("--version"),
        )
        .unwrap_or_else(|error| panic!("exit zero must survive runner policy: {error}"));
    assert_eq!(output.exit_code, Some(0));
    assert!(output.stdout.starts_with(b"rustc "));
    let output = SystemMimeCommandExecutor
        .run(
            &runner,
            Command::new("rustc").arg("--qubit-invalid-command-test-option"),
        )
        .unwrap_or_else(|error| panic!("numeric nonzero status must reach business policy: {error}"));
    assert!(output.exit_code.is_some_and(|code| code != 0));
    let error = match SystemMimeCommandExecutor.run(
        &runner.clone().max_output_bytes(1),
        Command::new("rustc").arg("--version"),
    ) {
        Ok(_) => panic!("strict output limit must fail"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), CommandErrorKind::OutputTruncated);
    let error = match SystemMimeCommandExecutor.run(&runner, Command::new("__qubit_mime_missing_test_executable__")) {
        Ok(_) => panic!("missing program must fail"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), CommandErrorKind::SpawnFailed);
}

#[test]
fn test_executor_preserves_capture_when_total_output_budget_is_exceeded() {
    let runner = CommandRunner::new(Duration::from_secs(10))
        .disable_logging(true)
        .max_total_output_bytes(8);
    let error = match SystemMimeCommandExecutor.run(&runner, Command::new("rustc").arg("--version")) {
        Err(error) => error,
        Ok(_) => panic!("rustc version output must exceed the small total budget"),
    };
    assert_eq!(error.kind(), CommandErrorKind::OutputLimitExceeded);
    assert!(error.output().is_none());
    let captured = error.captured_output().expect("partial output must be retained");
    assert!(!captured.stdout().is_empty());
}

#[cfg(test)]
mod managed_command_executor_tests {
    use std::collections::HashMap;
    use std::future::Future;
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;
    use std::sync::mpsc;
    use std::task::Context;
    use std::task::Poll;
    use std::task::Wake;
    use std::task::Waker;
    use std::time::Duration;

    use qubit_command::Command;
    use qubit_command::CommandRunner;
    use qubit_operation::ClockDomainId;
    use qubit_operation::ClockSource;
    use qubit_operation::Deadline;
    use qubit_operation::GroupOptions;
    use qubit_operation::MonotonicTime;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOptions;
    use qubit_operation::OperationOutcome;
    use qubit_operation::RecoveryPhase;
    use qubit_operation::ShutdownMode;
    use qubit_operation::TimerRegistration;

    use crate::InputExtent;
    use crate::MimeError;
    use crate::MimeOperationOptions;
    use crate::OwnedMimeInput;
    use crate::command_execution::AsyncMimeCommandExecutor;
    use crate::command_execution::FixedMimeExecutionPort;
    use crate::command_execution::ManagedCommandExecutor;
    use crate::command_execution::ManagedMimeCommand;
    use crate::command_execution::MimeCommandOutput;
    use crate::command_execution::MimeExecutionWorker;
    use crate::detector::MimeDetectorRegistry;
    use crate::detector::MimeExecutionPort;
    use crate::detector::MimeExecutionServices;
    use crate::detector::MimeRuntime;
    use crate::managed_temp_staging::ManagedStagingLimits;
    use crate::managed_temp_staging::stage_owned_in;

    struct ManualClockState {
        domain: ClockDomainId,
        ticks: std::sync::atomic::AtomicU64,
        next_id: std::sync::atomic::AtomicU64,
        registrations: Mutex<HashMap<u64, (u64, std::task::Waker)>>,
    }

    struct ManualClock(Arc<ManualClockState>);

    impl ManualClock {
        fn new() -> Arc<Self> {
            Arc::new(Self(Arc::new(ManualClockState {
                domain: ClockDomainId::new_unique(),
                ticks: std::sync::atomic::AtomicU64::new(0),
                next_id: std::sync::atomic::AtomicU64::new(1),
                registrations: Mutex::new(HashMap::new()),
            })))
        }

        fn advance_to(&self, ticks: u64) {
            self.0.ticks.store(ticks, std::sync::atomic::Ordering::SeqCst);
            let wakes = self
                .0
                .registrations
                .lock()
                .unwrap()
                .values()
                .filter_map(|(at, wake)| (*at <= ticks).then_some(wake.clone()))
                .collect::<Vec<_>>();
            for wake in wakes {
                wake.wake();
            }
        }
    }

    struct ManualTimer {
        clock: Arc<ManualClockState>,
        id: u64,
    }
    impl TimerRegistration for ManualTimer {
        fn cancel(self: Box<Self>) {
            self.clock.registrations.lock().unwrap().remove(&self.id);
        }
    }
    impl ClockSource for ManualClock {
        fn now(&self) -> MonotonicTime {
            MonotonicTime::new(self.0.domain, self.0.ticks.load(std::sync::atomic::Ordering::SeqCst))
        }
        fn register_deadline(&self, at: MonotonicTime, wake: std::task::Waker) -> Box<dyn TimerRegistration> {
            let id = self.0.next_id.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.0
                .registrations
                .lock()
                .unwrap()
                .insert(id, (at.ticks(), wake.clone()));
            if self.now().ticks() >= at.ticks() {
                wake.wake();
            }
            Box::new(ManualTimer {
                clock: Arc::clone(&self.0),
                id,
            })
        }
    }

    #[derive(Default)]
    struct DispatchState {
        saw_staging: bool,
        saw_command: bool,
        held_command: Option<Box<dyn FnOnce() + Send + 'static>>,
    }

    struct StagingThenHoldCommandPort {
        state: Mutex<DispatchState>,
        command_queued: mpsc::Sender<()>,
    }

    impl std::fmt::Debug for StagingThenHoldCommandPort {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("StagingThenHoldCommandPort")
        }
    }

    impl StagingThenHoldCommandPort {
        fn release_command(&self) {
            let job = self
                .state
                .lock()
                .unwrap()
                .held_command
                .take()
                .expect("command job is held");
            job();
        }
    }

    impl MimeExecutionPort for StagingThenHoldCommandPort {
        fn prepare(&self) -> Result<(), MimeError> {
            Ok(())
        }
        fn try_dispatch(
            &self,
            job: Box<dyn FnOnce() + Send + 'static>,
        ) -> Result<(), Box<dyn FnOnce() + Send + 'static>> {
            let mut state = self.state.lock().unwrap();
            if !state.saw_staging {
                state.saw_staging = true;
                drop(state);
                job();
                return Ok(());
            }
            if state.saw_command {
                drop(state);
                job();
                return Ok(());
            }
            state.saw_command = true;
            state.held_command = Some(job);
            drop(state);
            let _ = self.command_queued.send(());
            Ok(())
        }
        fn shutdown(&self) -> Result<(), MimeError> {
            Ok(())
        }
    }

    struct HoldCommandPort {
        held: Mutex<Option<Box<dyn FnOnce() + Send + 'static>>>,
        queued: mpsc::Sender<()>,
    }

    impl std::fmt::Debug for HoldCommandPort {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("HoldCommandPort")
        }
    }

    impl HoldCommandPort {
        fn destroy_held(&self) {
            drop(self.held.lock().unwrap().take().expect("accepted command is held"));
        }
    }

    impl MimeExecutionPort for HoldCommandPort {
        fn prepare(&self) -> Result<(), MimeError> {
            Ok(())
        }

        fn try_dispatch(
            &self,
            job: Box<dyn FnOnce() + Send + 'static>,
        ) -> Result<(), Box<dyn FnOnce() + Send + 'static>> {
            let mut slot = self.held.lock().unwrap();
            if slot.is_some() {
                drop(slot);
                job();
            } else {
                *slot = Some(job);
                drop(slot);
                let _ = self.queued.send(());
            }
            Ok(())
        }

        fn shutdown(&self) -> Result<(), MimeError> {
            Ok(())
        }
    }

    #[test]
    fn test_timeout_outcome_keeps_accepted_queued_command_owner_until_worker_returns() {
        use qubit_operation::OperationOutcome;
        use qubit_operation::StopReason;

        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 4 }).unwrap();
        let worker_port = Arc::new(FixedMimeExecutionPort::new(1, 4));
        let execution_port: Arc<dyn MimeExecutionPort> = worker_port.clone();
        let services = Arc::new(MimeExecutionServices::new(manager, group.clone(), execution_port, 128).unwrap());
        services.prepare().unwrap();

        let worker = MimeExecutionWorker::new(worker_port.clone());

        let operation_options = OperationOptions::default();
        let budget = services.staging_budget();
        let limits = ManagedStagingLimits {
            max_bytes: 64,
            budget: budget.clone(),
            worker: worker.clone(),
        };
        let read_tree = services.new_read_tree(64);
        let executor = ManagedCommandExecutor::new(worker);
        let runtime_group = group.clone();
        let (staged_path_tx, staged_path_rx) = mpsc::channel();
        let (child_pending_tx, child_pending_rx) = mpsc::channel();
        let (allow_command_tx, allow_command_rx) = mpsc::channel();
        let payload = b"queued command retains staged owner".to_vec();
        let expected_bytes = payload.len() as u64;
        let mut operation = block_on(group.try_submit_async(operation_options, move |scope| async move {
            let staged = stage_owned_in(
                &scope,
                OwnedMimeInput::bytes(Arc::from(payload), InputExtent::Complete),
                limits,
                read_tree,
            )
            .await?;
            staged_path_tx.send(staged.path()?).unwrap();
            allow_command_rx.recv().unwrap();
            let request = ManagedMimeCommand::new(
                CommandRunner::new(Duration::from_secs(10)).disable_logging(true),
                Command::new("must-not-start-after-timeout"),
                runtime_group,
            );
            let mut command_future = Box::pin(executor.run_in(&scope, request, staged));
            let mut pending_notified = false;
            let result = std::future::poll_fn(|cx| {
                let result = command_future.as_mut().poll(cx);
                if result.is_pending() && !pending_notified {
                    pending_notified = true;
                    child_pending_tx.send(()).unwrap();
                }
                result
            })
            .await;
            Ok::<_, MimeError>(result)
        }))
        .unwrap();

        let staged_path = staged_path_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("staging should finish before the command is accepted");
        let (blocker_started_tx, blocker_started_rx) = mpsc::channel();
        let (release_blocker_tx, release_blocker_rx) = mpsc::channel();
        assert!(
            worker_port
                .try_dispatch(Box::new(move || {
                    blocker_started_tx.send(()).unwrap();
                    release_blocker_rx.recv().unwrap();
                }))
                .is_ok()
        );
        blocker_started_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("controlled worker should be occupied after staging completes");
        allow_command_tx.send(()).unwrap();
        child_pending_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("managed command should be accepted and wait behind the blocker");
        assert!(
            staged_path.exists(),
            "staged owner remains live while command is queued"
        );
        assert_eq!(expected_bytes, budget.used_bytes());

        let _ = operation.request_stop(StopReason::Timeout);
        assert!(matches!(
            block_on(operation.wait_outcome()).unwrap(),
            OperationOutcome::Stopped(report) if report.reason == StopReason::Timeout
        ));

        let mut settlement = Box::pin(operation.wait_settlement());
        let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
        let mut context = Context::from_waker(&waker);
        assert!(
            matches!(settlement.as_mut().poll(&mut context), Poll::Pending),
            "settlement must wait for the accepted worker job to return"
        );
        assert!(
            staged_path.exists(),
            "timeout outcome must not release the queued owner's file"
        );
        assert_eq!(expected_bytes, budget.used_bytes(), "the staging lease remains held");

        release_blocker_tx.send(()).unwrap();
        let settled = block_on(settlement);
        assert!(settled.all_released());
        assert!(!staged_path.exists(), "worker return allows staged-file cleanup");
        assert_eq!(0, budget.used_bytes(), "settlement releases the staging lease");

        let shutdown = services.shutdown();
        let _ = block_on(shutdown.wait_services_settled()).unwrap();
        let _ = shutdown.stop_services().unwrap();
        let _ = block_on(shutdown.wait_group_shutdown()).unwrap();
    }

    #[test]
    fn test_accepted_command_destruction_reports_execution_failure_and_releases_input() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 2 }).unwrap();
        let (command_queued_tx, command_queued_rx) = mpsc::channel();
        let worker_port = Arc::new(HoldCommandPort {
            held: Mutex::new(None),
            queued: command_queued_tx,
        });
        let execution_port: Arc<dyn MimeExecutionPort> = worker_port.clone();
        let services = Arc::new(MimeExecutionServices::new(manager, group.clone(), execution_port, 128).unwrap());
        services.prepare().unwrap();

        let budget = services.staging_budget();
        let staging_port = Arc::new(FixedMimeExecutionPort::new(1, 4));
        staging_port.prepare().unwrap();
        let limits = ManagedStagingLimits {
            max_bytes: 64,
            budget: budget.clone(),
            worker: MimeExecutionWorker::new(staging_port.clone()),
        };
        let read_tree = services.new_read_tree(64);
        let runner_invocations = Arc::new(AtomicUsize::new(0));
        let executor = ManagedCommandExecutor::with_runner_invocations(
            MimeExecutionWorker::new(worker_port.clone()),
            Arc::clone(&runner_invocations),
        );
        let runtime_group = group.clone();
        let (staged_path_tx, staged_path_rx) = mpsc::channel();
        let (command_pending_tx, command_pending_rx) = mpsc::channel();
        let (result_tx, result_rx) = mpsc::channel();
        let mut operation = block_on(group.try_submit_async(Default::default(), move |scope| async move {
            let staged = stage_owned_in(
                &scope,
                OwnedMimeInput::bytes(Arc::from(b"accepted command input".as_slice()), InputExtent::Complete),
                limits,
                read_tree,
            )
            .await?;
            staged_path_tx.send(staged.path()?).unwrap();
            let request = ManagedMimeCommand::new(
                CommandRunner::new(Duration::from_secs(5)).disable_logging(true),
                Command::new("must-not-run"),
                runtime_group,
            );
            let mut command_future = Box::pin(executor.run_in(&scope, request, staged));
            let mut pending_notified = false;
            let result = std::future::poll_fn(|cx| {
                let result = command_future.as_mut().poll(cx);
                if result.is_pending() && !pending_notified {
                    pending_notified = true;
                    command_pending_tx.send(()).unwrap();
                }
                result
            })
            .await;
            result_tx.send(result).unwrap();
            Ok::<_, MimeError>(())
        }))
        .unwrap();

        let staged_path = staged_path_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        command_queued_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        command_pending_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("accepted command should be pending before the test destroys it");
        assert!(
            staged_path.exists(),
            "command input stays live until abandonment is confirmed"
        );
        worker_port.destroy_held();
        let command_result = result_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let outcome = block_on(operation.wait_outcome()).unwrap();
        assert!(
            matches!(
                command_result,
                Err(MimeError::OperationExecutionFailed(ref failure))
                    if failure.summary.to_string().contains("accepted but discarded")
            ),
            "unexpected command error: {:?}",
            command_result.as_ref().err()
        );
        assert!(matches!(outcome, OperationOutcome::Succeeded(())));
        assert_eq!(0, runner_invocations.load(Ordering::SeqCst));
        assert!(block_on(operation.wait_settlement()).all_released());
        assert!(!staged_path.exists(), "confirmed abandonment permits staged cleanup");
        assert_eq!(0, budget.used_bytes());
        staging_port.shutdown().unwrap();

        let shutdown = services.shutdown();
        let _ = block_on(shutdown.wait_services_settled()).unwrap();
        let _ = shutdown.stop_services().unwrap();
        let _ = block_on(shutdown.wait_group_shutdown()).unwrap();
    }

    #[test]
    fn test_real_context_deadline_stops_parent_before_queued_command_settles() {
        use qubit_operation::StopReason;

        let clock = ManualClock::new();
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 4 }).unwrap();
        let (command_queued_tx, command_queued_rx) = mpsc::channel();
        let worker_port = Arc::new(StagingThenHoldCommandPort {
            state: Mutex::new(DispatchState::default()),
            command_queued: command_queued_tx,
        });
        let execution_port: Arc<dyn MimeExecutionPort> = worker_port.clone();
        let services = Arc::new(MimeExecutionServices::new(manager, group, execution_port, 128).unwrap());
        services.prepare().unwrap();

        let budget = services.staging_budget();
        let runtime = MimeRuntime::builtin()
            .unwrap()
            .with_execution_services((*services).clone())
            .with_async_provider_selection(
                &MimeDetectorRegistry::builtin(),
                &qubit_spi::ProviderSelection::named("file").unwrap(),
            )
            .unwrap();
        let payload: Arc<[u8]> = Arc::from(b"real MIME deadline retains staged input".as_slice());
        let expected_bytes = payload.len() as u64;
        let options = MimeOperationOptions {
            operation: OperationOptions {
                context: qubit_operation::ContextOptions {
                    clock: Some(clock.clone()),
                    deadline: Some(Deadline::at(MonotonicTime::new(clock.0.domain, 10))),
                    ..Default::default()
                },
                ..Default::default()
            },
            ..Default::default()
        };
        let mut operation = runtime
            .try_submit_detection(
                OwnedMimeInput::Bytes {
                    content: payload,
                    extent: InputExtent::Complete,
                    filename: None,
                },
                options,
            )
            .unwrap_or_else(|_| panic!("public MIME runtime accepts the managed file detector operation"));
        command_queued_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("staging completes and actual command dispatch is held");
        assert_eq!(
            expected_bytes,
            budget.used_bytes(),
            "command job retains its staged input lease"
        );
        assert!(
            !clock.0.registrations.lock().unwrap().is_empty(),
            "production MIME detect registered its context deadline"
        );

        clock.advance_to(10);
        let outcome = block_on_timeout(operation.wait_outcome(), Duration::from_secs(2))
            .expect("production MIME context deadline wakes parent outcome")
            .unwrap();
        assert!(matches!(outcome, OperationOutcome::Stopped(report) if report.reason == StopReason::Timeout));
        assert!(
            clock.0.registrations.lock().unwrap().is_empty(),
            "production deadline signal cancels its timer registration after firing"
        );

        let mut settlement = Box::pin(operation.wait_settlement());
        let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
        let mut context = Context::from_waker(&waker);
        assert!(
            matches!(settlement.as_mut().poll(&mut context), Poll::Pending),
            "worker still owns command settlement"
        );
        assert!(
            worker_port.state.lock().unwrap().held_command.is_some(),
            "command remains queued through outcome publication"
        );
        assert_eq!(expected_bytes, budget.used_bytes());

        worker_port.release_command();
        let settled = block_on_timeout(settlement, Duration::from_secs(2)).expect("released command settles");
        assert!(settled.all_released());
        assert_eq!(0, budget.used_bytes());

        let shutdown = services.shutdown();
        let _ = block_on_timeout(shutdown.wait_services_settled(), Duration::from_secs(2)).unwrap();
        let _ = shutdown.stop_services().unwrap();
        let _ = block_on_timeout(shutdown.wait_group_shutdown(), Duration::from_secs(2)).unwrap();
    }

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

    fn block_on_timeout<F: Future>(future: F, timeout: Duration) -> Result<F::Output, &'static str> {
        let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
        let mut context = Context::from_waker(&waker);
        let mut future = std::pin::pin!(future);
        let deadline = std::time::Instant::now() + timeout;
        loop {
            if let Poll::Ready(value) = future.as_mut().poll(&mut context) {
                return Ok(value);
            }
            let now = std::time::Instant::now();
            if now >= deadline {
                return Err("future did not complete before test timeout");
            }
            std::thread::park_timeout(deadline - now);
        }
    }

    fn outcome_kind<T, E>(outcome: &OperationOutcome<T, E>) -> &'static str {
        match outcome {
            OperationOutcome::Succeeded(_) => "succeeded",
            OperationOutcome::Failed(_) => "failed",
            OperationOutcome::Stopped(_) => "stopped",
            OperationOutcome::Panicked(_) => "panicked",
            OperationOutcome::ExecutionFailed(_) => "execution failed",
            OperationOutcome::ChildFailed(_) => "child failed",
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
        let executor = ManagedCommandExecutor::new(MimeExecutionWorker::new(port.clone()));
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
        let _ = block_on(shutdown.wait_services_settled()).unwrap();
        let _ = shutdown.stop_services().unwrap();
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
            matches!(failure, OperationOutcome::Failed(MimeError::Command(_))),
            "the parent operation reports the typed command error"
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
        let stopped = matches!(outcome, OperationOutcome::Stopped(_));
        assert!(
            stopped,
            "a parent stop should surface the cancelled required child: {:?}",
            std::mem::discriminant(&outcome)
        );
    }

    #[test]
    fn test_managed_executor_reuses_parent_group_capacity() {
        let outcome = run_managed_command(Command::new("rustc").arg("--version"), 1, false, false);
        assert!(matches!(
            outcome,
            OperationOutcome::Succeeded(ref output)
                if output.exit_code == Some(0) && output.stdout.starts_with(b"rustc ")
        ));
    }

    #[test]
    fn test_managed_executor_rejects_group_from_another_manager() {
        let outcome = run_managed_command(Command::new("must-not-start"), 4, true, false);
        assert!(matches!(outcome, OperationOutcome::Failed(MimeError::GroupBinding(_))));
    }

    #[test]
    fn test_managed_executor_maps_full_worker_queue_to_busy_and_releases_input() {
        let _recovery_port_guard = crate::managed_temp_staging::recovery_cleanup_test_guard();

        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 2 }).unwrap();
        let port = Arc::new(FixedMimeExecutionPort::new(1, 1));
        let services = Arc::new(MimeExecutionServices::new(manager, group.clone(), port.clone(), 128).unwrap());
        services.prepare().unwrap();

        let (staged_tx, staged_rx) = mpsc::channel();
        let (dispatch_tx, dispatch_rx) = mpsc::channel();
        let (result_tx, result_rx) = mpsc::channel();
        let limits = ManagedStagingLimits {
            max_bytes: 64,
            budget: services.staging_budget(),
            worker: MimeExecutionWorker::new(port.clone()),
        };
        let read_tree = services.new_read_tree(64);
        let executor = ManagedCommandExecutor::new(MimeExecutionWorker::new(port.clone()));
        let runtime_group = group.clone();
        let mut operation = block_on(group.try_submit_async(Default::default(), move |scope| async move {
            let staged = stage_owned_in(
                &scope,
                OwnedMimeInput::bytes(Arc::from(b"queue full input".as_slice()), InputExtent::Complete),
                limits,
                read_tree,
            )
            .await?;
            staged_tx.send(staged.path()?).unwrap();
            dispatch_rx.recv().unwrap();
            let request = ManagedMimeCommand::new(
                CommandRunner::new(Duration::from_secs(5)).disable_logging(true),
                Command::new("must-not-run"),
                runtime_group,
            );
            let outcome = executor.run_in(&scope, request, staged).await;
            result_tx.send(outcome).unwrap();
            Ok::<_, MimeError>(())
        }))
        .unwrap();
        let staged_path = staged_rx.recv().unwrap();
        assert!(staged_path.exists(), "staged input exists while its operation owns it");

        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        assert!(
            MimeExecutionWorker::new(port.clone())
                .try_dispatch(Box::new(move || {
                    started_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                }))
                .is_ok()
        );
        started_rx.recv().unwrap();
        let (queued_tx, queued_rx) = mpsc::channel();
        assert!(
            MimeExecutionWorker::new(port.clone())
                .try_dispatch(Box::new(move || queued_tx.send(()).unwrap()))
                .is_ok()
        );
        dispatch_tx.send(()).unwrap();

        let outcome = block_on(operation.wait_outcome()).unwrap();
        let command_result = result_rx.recv().unwrap();
        let settlement = block_on(operation.wait_settlement());
        assert_eq!(1, settlement.recovery_count, "full worker queue retains cleanup");
        release_tx.send(()).unwrap();
        queued_rx.recv().unwrap();
        let shutdown = services.shutdown();
        let recoveries = shutdown.recovery_handles_snapshot();
        assert_eq!(1, recoveries.len());
        let recovery = recoveries[0].try_retry().unwrap().wait().unwrap();
        assert_eq!(RecoveryPhase::Resolved, recovery.recovery.phase);
        assert_eq!(0, shutdown.snapshot().unresolved_recoveries);
        let _ = block_on(shutdown.wait_services_settled()).unwrap();
        let _ = shutdown.stop_services().unwrap();
        let _ = block_on(shutdown.wait_group_shutdown()).unwrap();
        assert!(!staged_path.exists(), "resolved cleanup removes the staged input");
        if let OperationOutcome::Failed(error) = &outcome {
            panic!("parent operation failed before returning the command result: {error:?}");
        }
        assert!(matches!(command_result, Err(MimeError::CommandQueueBusy)));
        assert!(matches!(outcome, OperationOutcome::Succeeded(())));
    }

    #[test]
    fn test_managed_executor_returns_input_when_group_cancels_external_admission() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 2 }).unwrap();
        let port = Arc::new(FixedMimeExecutionPort::new(1, 2));
        let services = Arc::new(MimeExecutionServices::new(manager, group.clone(), port.clone(), 128).unwrap());
        services.prepare().unwrap();
        let limits = ManagedStagingLimits {
            max_bytes: 64,
            budget: services.staging_budget(),
            worker: MimeExecutionWorker::new(port.clone()),
        };
        let read_tree = services.new_read_tree(64);
        let executor = ManagedCommandExecutor::new(MimeExecutionWorker::new(port.clone()));
        let runtime_group = group.clone();
        let cancel_group = group.clone();
        let (result_tx, result_rx) = mpsc::channel();
        let mut active_parent = block_on(group.try_submit_async(Default::default(), move |scope| async move {
            let staged = stage_owned_in(
                &scope,
                OwnedMimeInput::bytes(Arc::from(b"closed admission input".as_slice()), InputExtent::Complete),
                limits,
                read_tree,
            )
            .await?;
            let _group_shutdown = cancel_group.request_shutdown(ShutdownMode::Cancel);
            let request = ManagedMimeCommand::new(
                CommandRunner::new(Duration::from_secs(5)).disable_logging(true),
                Command::new("must-not-run"),
                runtime_group,
            );
            result_tx.send(executor.run_in(&scope, request, staged).await).unwrap();
            Ok::<_, MimeError>(())
        }))
        .unwrap();

        let outcome = block_on(active_parent.wait_outcome()).unwrap();
        let command_result = result_rx.recv().unwrap();
        assert!(block_on(active_parent.wait_settlement()).all_released());
        port.shutdown().unwrap();
        assert!(
            matches!(command_result, Err(MimeError::OperationAdmission(_))),
            "closed external admission rejects the child"
        );
        assert!(
            matches!(&outcome, OperationOutcome::Stopped(_)),
            "group cancellation stops the parent after child rejection, got {}",
            outcome_kind(&outcome)
        );
    }
}
