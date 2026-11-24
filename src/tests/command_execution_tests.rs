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
    use std::future::Future;
    use std::sync::Arc;
    use std::sync::mpsc;
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
    use qubit_operation::RecoveryPhase;
    use qubit_operation::ShutdownMode;

    use crate::InputExtent;
    use crate::MimeError;
    use crate::OwnedMimeInput;
    use crate::command_execution::AsyncMimeCommandExecutor;
    use crate::command_execution::FixedMimeExecutionPort;
    use crate::command_execution::ManagedCommandExecutor;
    use crate::command_execution::ManagedMimeCommand;
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
        assert!(matches!(outcome, OperationOutcome::ChildFailed(_)));
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
