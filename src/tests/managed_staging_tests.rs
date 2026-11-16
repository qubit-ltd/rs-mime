// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Focused checks for staging bounds and fixed-worker admission.

use std::future::Future;
use std::sync::{
    Arc, Barrier,
    atomic::{AtomicBool, Ordering},
};
use std::task::{Context, Poll, Wake, Waker};

use crate::command_execution::FixedMimeExecutionPort;
use crate::command_execution::{
    AsyncMimeCommandExecutor, ManagedMimeCommand, MimeCommandOutput, MimeExecutionWorker,
};
use crate::detector::{MimeExecutionPort, MimeExecutionServices};
use crate::managed_temp_staging::{
    ManagedStagingLimits, StagedMimeInput, commit_staged, sentinel_read_limit, stage_owned_in,
    within_staging_limit,
};
use crate::{InputExtent, OwnedMimeInput};

struct ThreadWake(std::thread::Thread);
impl Wake for ThreadWake {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
    fn wake_by_ref(self: &Arc<Self>) {
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

#[test]
fn managed_staging_limit_boundaries() {
    let limit = 8;
    assert!(within_staging_limit(limit - 1, limit));
    assert!(within_staging_limit(limit, limit));
    assert!(!within_staging_limit(limit + 1, limit));
}

#[test]
fn managed_staging_zero_limit() {
    assert!(within_staging_limit(0, 0));
    assert!(!within_staging_limit(1, 0));
}

#[test]
fn managed_staging_checked_sentinel_arithmetic() {
    assert_eq!(sentinel_read_limit(8), Some(9));
    assert_eq!(sentinel_read_limit(u64::MAX), None);
}

#[test]
fn managed_staging_cancel_before_create() {
    let worker = FixedMimeExecutionPort::new(1, 1);
    let called = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&called);
    let job: Box<dyn FnOnce() + Send> = Box::new(move || flag.store(true, Ordering::SeqCst));
    let returned = worker
        .try_dispatch(job)
        .expect_err("unprepared worker rejects without running");
    drop(returned);
    assert!(!called.load(Ordering::SeqCst));
}

#[test]
fn managed_staging_worker_queue_is_bounded_and_shutdown_joins() {
    let worker = FixedMimeExecutionPort::new(1, 1);
    worker.prepare().unwrap();
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let entered_job = Arc::clone(&entered);
    let release_job = Arc::clone(&release);
    assert!(
        worker
            .try_dispatch(Box::new(move || {
                entered_job.wait();
                release_job.wait();
            }))
            .is_ok()
    );
    entered.wait();
    assert!(worker.try_dispatch(Box::new(|| {})).is_ok());
    assert!(worker.try_dispatch(Box::new(|| {})).is_err());
    let release_thread = std::thread::spawn(move || release.wait());
    worker.shutdown().unwrap();
    release_thread.join().unwrap();
    assert!(worker.try_dispatch(Box::new(|| {})).is_err());
}

#[test]
fn managed_staging_owner_waits_for_scope_end_and_all_references() {
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let group = manager
        .group(qubit_operation::GroupOptions { capacity: 4 })
        .unwrap();
    let port = Arc::new(FixedMimeExecutionPort::new(1, 4));
    let services = MimeExecutionServices::new(manager, group.clone(), port.clone(), 1024).unwrap();
    services.prepare().unwrap();
    let limits = ManagedStagingLimits {
        max_bytes: 128,
        budget: services.staging_budget(),
        worker: crate::command_execution::MimeExecutionWorker::new(port),
    };
    let input = OwnedMimeInput::bytes(
        Arc::from(b"owned staging payload".as_slice()),
        InputExtent::Complete,
    );
    let mut handle = block_on(
        group.try_submit_async(Default::default(), move |scope| async move {
            stage_owned_in(&scope, input, limits).await
        }),
    )
    .unwrap();
    let resource = match block_on(handle.wait_outcome()).unwrap() {
        qubit_operation::OperationOutcome::Succeeded(resource) => resource,
        _ => panic!("staging operation did not succeed"),
    };
    let path = resource.path().unwrap();
    assert!(
        path.exists(),
        "resource ref keeps the staged file alive after body exit"
    );
    drop(resource);
    let report = block_on(handle.wait_settlement());
    assert!(report.all_released());
    assert!(
        !path.exists(),
        "scope end plus final reference drop triggers cleanup"
    );
    let shutdown = services.shutdown();
    block_on(shutdown.wait_services_settled()).unwrap();
    shutdown.stop_services().unwrap();
}

struct EarlyOutcomeCommand {
    helper_input: Arc<
        std::sync::Mutex<
            Option<qubit_operation::ResourceRef<crate::managed_temp_staging::StagedMimeInput>>,
        >,
    >,
}
impl std::fmt::Debug for EarlyOutcomeCommand {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("EarlyOutcomeCommand")
    }
}

impl AsyncMimeCommandExecutor for EarlyOutcomeCommand {
    fn run_in<'a>(
        &'a self,
        _scope: &'a qubit_operation::OperationScope,
        _command: ManagedMimeCommand,
        input: qubit_operation::ResourceRef<crate::managed_temp_staging::StagedMimeInput>,
    ) -> crate::MimeFuture<'a, MimeCommandOutput> {
        *self.helper_input.lock().unwrap() = Some(input);
        Box::pin(async {
            Ok(MimeCommandOutput {
                exit_code: Some(0),
                stdout: Vec::new(),
                stdout_truncated: false,
                stdout_complete: true,
            })
        })
    }
}

#[test]
fn managed_staging_outcome_precedes_helper_input_release() {
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let group = manager
        .group(qubit_operation::GroupOptions { capacity: 4 })
        .unwrap();
    let port = Arc::new(FixedMimeExecutionPort::new(1, 4));
    let services = MimeExecutionServices::new(manager, group.clone(), port.clone(), 1024).unwrap();
    services.prepare().unwrap();
    let limits = ManagedStagingLimits {
        max_bytes: 128,
        budget: services.staging_budget(),
        worker: MimeExecutionWorker::new(port),
    };
    let helper_input = Arc::new(std::sync::Mutex::new(None));
    let executor = Arc::new(EarlyOutcomeCommand {
        helper_input: Arc::clone(&helper_input),
    });
    let command_group = group.clone();
    let input = OwnedMimeInput::bytes(Arc::from(b"early result".as_slice()), InputExtent::Complete);
    let mut handle = block_on(group.try_submit_async(
        Default::default(),
        move |scope| async move {
            let staged = stage_owned_in(&scope, input, limits).await?;
            let request = ManagedMimeCommand::new(
                qubit_command::CommandRunner::new(std::time::Duration::from_secs(1)),
                qubit_command::Command::new("unused-by-fake"),
                command_group,
            );
            executor.run_in(&scope, request, staged).await
        },
    ))
    .unwrap();
    assert!(matches!(
        block_on(handle.wait_outcome()).unwrap(),
        qubit_operation::OperationOutcome::Succeeded(_)
    ));
    let held = helper_input.lock().unwrap().as_ref().unwrap().clone();
    let path = held.path().unwrap();
    assert!(
        path.exists(),
        "the outcome does not release an input still held by a helper"
    );
    drop(held);
    helper_input.lock().unwrap().take();
    block_on(handle.wait_settlement());
    assert!(
        !path.exists(),
        "the temp is cleaned after the helper returns its final reference"
    );
    let shutdown = services.shutdown();
    block_on(shutdown.wait_services_settled()).unwrap();
    shutdown.stop_services().unwrap();
}

#[cfg(unix)]
#[test]
fn managed_staging_cleanup_failure_retains_owner_for_retry() {
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let group = manager
        .group(qubit_operation::GroupOptions { capacity: 4 })
        .unwrap();
    let port = Arc::new(FixedMimeExecutionPort::new(1, 4));
    let services = MimeExecutionServices::new(manager, group.clone(), port.clone(), 1024).unwrap();
    services.prepare().unwrap();
    let limits = ManagedStagingLimits {
        max_bytes: 128,
        budget: services.staging_budget(),
        worker: MimeExecutionWorker::new(port),
    };
    let input = OwnedMimeInput::bytes(
        Arc::from(b"retry cleanup".as_slice()),
        InputExtent::Complete,
    );
    let mut handle = block_on(
        group.try_submit_async(Default::default(), move |scope| async move {
            stage_owned_in(&scope, input, limits).await
        }),
    )
    .unwrap();
    let resource = match block_on(handle.wait_outcome()).unwrap() {
        qubit_operation::OperationOutcome::Succeeded(resource) => resource,
        _ => panic!("staging operation did not succeed"),
    };
    let path = resource.path().unwrap();
    let sandbox = path.parent().unwrap().to_path_buf();
    let blocker = sandbox.join("cleanup-blocker");
    std::fs::write(&blocker, b"keep sandbox nonempty").unwrap();
    drop(resource);

    let report = block_on(handle.wait_settlement());
    assert_eq!(
        report.status,
        qubit_operation::SettlementStatus::RecoveryRequired
    );
    assert!(
        report.recoveries.len() == 1,
        "failed cleanup keeps one recovery owner"
    );
    assert!(
        !path.exists(),
        "cleanup removed the file but retained its sandbox owner"
    );
    std::fs::remove_file(blocker).unwrap();
    let recovery = report.recoveries[0].clone();
    let attempt = recovery.try_retry().unwrap();
    let retry_report = block_on(attempt.wait_async());
    assert!(
        retry_report.failures.is_empty(),
        "retry should release the retained sandbox owner"
    );
    assert!(!sandbox.exists(), "retry removes the retained sandbox");
    let shutdown = services.shutdown();
    block_on(shutdown.wait_services_settled()).unwrap();
    shutdown.stop_services().unwrap();
}

#[test]
fn managed_staging_commit_failure_retains_partial_owner_and_permit() {
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let group = manager
        .group(qubit_operation::GroupOptions { capacity: 4 })
        .unwrap();
    let port = Arc::new(FixedMimeExecutionPort::new(1, 4));
    let services = MimeExecutionServices::new(manager, group.clone(), port.clone(), 1024).unwrap();
    services.prepare().unwrap();
    let worker = MimeExecutionWorker::new(port);
    let reservation_slot = Arc::new(std::sync::Mutex::new(None));
    let slot = Arc::clone(&reservation_slot);
    let mut handle = block_on(group.try_submit_async(
        Default::default(),
        move |scope| async move {
            *slot.lock().unwrap() = Some(scope.reserve_resource().unwrap());
            Ok::<(), crate::MimeError>(())
        },
    ))
    .unwrap();
    assert!(matches!(
        block_on(handle.wait_outcome()).unwrap(),
        qubit_operation::OperationOutcome::Succeeded(())
    ));

    let mut temp = qubit_local_files::LocalFileSystem::host()
        .unwrap()
        .create_temp_file()
        .unwrap();
    use std::io::Write;
    temp.as_file_mut()
        .unwrap()
        .write_all(b"partial owner")
        .unwrap();
    let path = temp.path().to_path_buf();
    let permit = services.staging_budget().try_reserve(13).unwrap();
    let staged = StagedMimeInput::new(temp, permit, worker);
    let reservation = reservation_slot.lock().unwrap().take().unwrap();
    assert!(matches!(
        commit_staged(reservation, staged),
        Err(crate::MimeError::ExecutionServicesRequired)
    ));
    block_on(handle.wait_settlement());
    assert!(
        !path.exists(),
        "failed commit routes the partial temp to reserved cleanup"
    );
    assert!(
        services.staging_budget().try_reserve(13).is_some(),
        "cleanup releases the staged-byte permit"
    );
    let shutdown = services.shutdown();
    block_on(shutdown.wait_services_settled()).unwrap();
    shutdown.stop_services().unwrap();
}

#[test]
fn managed_staging_unconsumed_outcome_does_not_hold_temp() {
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let group = manager
        .group(qubit_operation::GroupOptions { capacity: 4 })
        .unwrap();
    let port = Arc::new(FixedMimeExecutionPort::new(1, 4));
    let services = MimeExecutionServices::new(manager, group.clone(), port.clone(), 1024).unwrap();
    services.prepare().unwrap();
    let limits = ManagedStagingLimits {
        max_bytes: 128,
        budget: services.staging_budget(),
        worker: MimeExecutionWorker::new(port),
    };
    let path_slot = Arc::new(std::sync::Mutex::new(None));
    let slot = Arc::clone(&path_slot);
    let input = OwnedMimeInput::bytes(
        Arc::from(b"no resource in outcome".as_slice()),
        InputExtent::Complete,
    );
    let handle = block_on(
        group.try_submit_async(Default::default(), move |scope| async move {
            let staged = stage_owned_in(&scope, input, limits).await?;
            *slot.lock().unwrap() = Some(staged.path()?);
            drop(staged);
            Ok::<(), crate::MimeError>(())
        }),
    )
    .unwrap();
    // Deliberately never consume the unit outcome. It contains no resource ref.
    let report = block_on(handle.wait_settlement());
    assert!(report.all_released());
    assert!(!path_slot.lock().unwrap().as_ref().unwrap().exists());
    let shutdown = services.shutdown();
    block_on(shutdown.wait_services_settled()).unwrap();
    shutdown.stop_services().unwrap();
}
