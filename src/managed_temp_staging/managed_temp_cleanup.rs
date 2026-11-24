// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Async cleanup owner for staged temporary files.

use std::io::Error;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use qubit_operation::AsyncCleanup;
use qubit_operation::CleanupContext;
use qubit_operation::CleanupFuture;
use qubit_operation::CleanupOutcome;
use qubit_operation::ResourceId;

use super::StagedMimeInput;
use super::cleanup_failure;
use super::completion::completion_pair;
use super::panic_message;
use crate::MimeError;
use crate::command_execution::FixedMimeExecutionPort;
use crate::detector::MimeExecutionPort;

/// Cleanup adapter that dispatches staged-file deletion to the MIME worker.
pub(super) struct ManagedTempCleanup {
    /// Resource ID recorded for cleanup failure diagnostics.
    pub(super) resource_id: Arc<super::resource_identity::ResourceIdentity>,
    /// Number of cleanup attempts made through this policy.
    pub(super) attempts: AtomicUsize,
}

/// Process-owned bounded executor kept alive independently of MIME services.
static RECOVERY_CLEANUP_PORT: OnceLock<Arc<FixedMimeExecutionPort>> = OnceLock::new();
#[cfg(test)]
static TEST_RECOVERY_PORT_LOCK: Mutex<()> = Mutex::new(());

/// Serializes tests that dispatch retries through the process recovery
/// executor.
#[cfg(test)]
pub(crate) fn recovery_cleanup_test_guard() -> std::sync::MutexGuard<'static, ()> {
    TEST_RECOVERY_PORT_LOCK
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
}

/// Gets the process recovery executor and ensures its workers are ready.
fn recovery_cleanup_port() -> Result<&'static Arc<FixedMimeExecutionPort>, MimeError> {
    let port = RECOVERY_CLEANUP_PORT.get_or_init(|| Arc::new(FixedMimeExecutionPort::new(1, 16)));
    port.prepare()?;
    Ok(port)
}
impl AsyncCleanup<StagedMimeInput> for ManagedTempCleanup {
    /// Removes the staged file and returns either successful cleanup or the
    /// original resource for explicit recovery.
    ///
    /// # Parameters
    /// - `self`: Shared cleanup adapter.
    /// - `resource`: Staged file whose owner is being removed.
    /// - `_context`: Cleanup context supplied by the operation manager.
    ///
    /// # Returns
    /// A future resolving to released cleanup or a recoverable failure.
    ///
    /// # Panics
    /// Panics if a rejected worker dispatch has already transferred resource
    /// ownership, violating the worker contract.
    fn cleanup(self: Arc<Self>, resource: StagedMimeInput, _context: CleanupContext) -> CleanupFuture<StagedMimeInput> {
        let retry = self.attempts.fetch_add(1, Ordering::AcqRel) > 0;
        let identity = Arc::clone(&self.resource_id);
        Box::pin(async move {
            let id = identity.wait().await;
            cleanup_with_id(resource, retry, id).await
        })
    }
}

/// Schedules cleanup after the resource identity has been published.
///
/// The returned future preserves ownership when dispatch fails so recovery can
/// retry cleanup with the same registered resource ID.
///
/// # Parameters
/// - `resource`: staged file whose native owner cleanup will release.
/// - `retry`: whether this cleanup follows an earlier failed attempt.
/// - `resource_id`: recovery ID used to report any cleanup failure.
///
/// # Returns
/// A future resolving to cleanup success or a failure that still owns resource.
fn cleanup_with_id(resource: StagedMimeInput, retry: bool, resource_id: ResourceId) -> CleanupFuture<StagedMimeInput> {
    let worker = resource.worker.clone();
    let cleanup_id = resource_id;
    let retained = Arc::new(Mutex::new(Some(resource)));
    let for_job = Arc::clone(&retained);
    let (sender, receiver) = completion_pair();
    let job = Box::new(move || {
        let Some(mut resource) = for_job.lock().unwrap_or_else(|poison| poison.into_inner()).take() else {
            return;
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            resource
                .owner
                .get_mut()
                .unwrap_or_else(|poison| poison.into_inner())
                .cleanup()
                .map_err(Box::new)
        }));
        let outcome = match result {
            Ok(Ok(())) => {
                // Return staged-byte capacity only after cleanup has
                // confirmed the native owner is gone. Clearing explicitly
                // makes budget release coincide with that evidence.
                resource
                    .permits
                    .get_mut()
                    .unwrap_or_else(|poison| poison.into_inner())
                    .clear();
                CleanupOutcome::Released
            }
            Ok(Err(error)) => CleanupOutcome::Failed {
                failure: cleanup_failure(cleanup_id, Arc::new(error)),
                resource,
            },
            Err(payload) => CleanupOutcome::Failed {
                failure: cleanup_failure(cleanup_id, Arc::new(Error::other(panic_message(payload.as_ref())))),
                resource,
            },
        };
        sender.complete(outcome);
    });
    if let Err(job) = worker.try_dispatch(job) {
        let mut failure_text = "MIME worker rejected cleanup job".to_owned();
        if retry {
            match recovery_cleanup_port() {
                Ok(port) => match port.try_dispatch(job) {
                    Ok(()) => return Box::pin(receiver),
                    Err(job) => {
                        drop(job);
                        failure_text = "MIME worker and recovery cleanup port rejected cleanup job".to_owned();
                    }
                },
                Err(error) => {
                    drop(job);
                    failure_text = format!(
                        "MIME worker rejected cleanup job and recovery cleanup port initialization failed: {error}"
                    );
                }
            }
        } else {
            drop(job);
        }
        // If fallback initialization or dispatch failed, retrieve the
        // original unexecuted job's resource and preserve it for another
        // recovery attempt.
        let resource = retained
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .take()
            .expect("rejected cleanup job has not taken ownership");
        let failure = cleanup_failure(cleanup_id, Arc::new(Error::other(failure_text)));
        return Box::pin(async move { CleanupOutcome::Failed { failure, resource } });
    }
    Box::pin(receiver)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::future::Future;
    use std::pin;
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::sync::atomic::AtomicU64;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;
    use std::task::Context;
    use std::task::Poll;
    use std::task::Wake;
    use std::task::Waker;
    use std::thread;
    use std::thread::Thread;

    use qubit_local_files::LocalFileSystem;
    use qubit_operation::AsyncCleanup;
    use qubit_operation::CleanupContext;
    use qubit_operation::CleanupOutcome;
    use qubit_operation::CleanupStage;
    use qubit_operation::ContextOptions;
    use qubit_operation::GroupOptions;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOutcome;
    use qubit_operation::RecoveryPhase;
    use qubit_operation::RecoveryStore;
    use qubit_operation::RecoveryStoreOptions;

    use super::super::resource_identity::ResourceIdentity;
    use super::super::staged_temp_owner::StagedTempOwner;
    use super::ManagedTempCleanup;
    use super::recovery_cleanup_port;
    use super::recovery_cleanup_test_guard;
    use crate::MimeError;
    use crate::command_execution::FixedMimeExecutionPort;
    use crate::command_execution::MimeExecutionWorker;
    use crate::detector::MimeExecutionPort;
    use crate::detector::MimeExecutionServices;
    use crate::managed_temp_staging::StagedMimeInput;
    use crate::managed_temp_staging::commit_staged;

    /// Wakes the thread blocked while polling a pending test future.
    struct ThreadWake(Thread);

    impl Wake for ThreadWake {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }

    /// Polls a future synchronously, parking this thread between wakeups.
    fn block_on<F: Future>(future: F) -> F::Output {
        let waker = Waker::from(Arc::new(ThreadWake(thread::current())));
        let mut context = Context::from_waker(&waker);
        let mut future = pin::pin!(future);
        loop {
            match future.as_mut().poll(&mut context) {
                Poll::Ready(value) => return value,
                Poll::Pending => thread::park(),
            }
        }
    }

    #[derive(Debug)]
    struct RejectingPort;

    impl MimeExecutionPort for RejectingPort {
        fn prepare(&self) -> Result<(), MimeError> {
            Ok(())
        }

        fn shutdown(&self) -> Result<(), MimeError> {
            Ok(())
        }
    }

    #[test]
    fn test_cleanup_waits_for_identity_before_dispatch_and_keeps_owner() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 1 }).unwrap();
        let port = Arc::new(FixedMimeExecutionPort::new(1, 1));
        port.prepare().unwrap();
        let worker = MimeExecutionWorker::new(port);
        let mut operation = block_on(group.try_submit_async(Default::default(), move |scope| async move {
            let owner = LocalFileSystem::host().unwrap().create_temp_file().unwrap();
            commit_staged(
                scope.reserve_resource().unwrap(),
                StagedMimeInput {
                    owner: Mutex::new(StagedTempOwner::File { owner }),
                    permits: Mutex::new(Vec::new()),
                    worker,
                    staged_bytes: AtomicU64::new(0),
                    cleanup_id: Arc::new(ResourceIdentity::default()),
                },
            )
        }))
        .unwrap();
        let registered = match block_on(operation.wait_outcome()).unwrap() {
            OperationOutcome::Succeeded(resource) => resource,
            _ => panic!("resource identity fixture should succeed"),
        };

        let owner = LocalFileSystem::host().unwrap().create_temp_file().unwrap();
        let path = owner.path().to_path_buf();
        let identity = Arc::new(ResourceIdentity::default());
        let resource = StagedMimeInput {
            owner: Mutex::new(StagedTempOwner::File { owner }),
            permits: Mutex::new(Vec::new()),
            worker: MimeExecutionWorker::new(Arc::new(RejectingPort)),
            staged_bytes: AtomicU64::new(0),
            cleanup_id: Arc::clone(&identity),
        };
        let cleanup = Arc::new(ManagedTempCleanup {
            resource_id: Arc::clone(&identity),
            attempts: AtomicUsize::new(0),
        });
        let mut future = pin::pin!(cleanup.cleanup(resource, CleanupContext::new(ContextOptions::default()).unwrap()));
        let waker = Waker::from(Arc::new(ThreadWake(thread::current())));
        let mut context = Context::from_waker(&waker);
        assert!(future.as_mut().poll(&mut context).is_pending());
        assert!(path.exists(), "pending cleanup must retain the native owner");
        identity.publish(registered.id());
        match future.as_mut().poll(&mut context) {
            Poll::Ready(CleanupOutcome::Failed { failure, mut resource }) => {
                assert_eq!(registered.id(), failure.resource_id);
                assert!(path.exists(), "failed dispatch must preserve the owner");
                resource.owner.get_mut().unwrap().cleanup().unwrap();
            }
            _ => panic!("identity publication must wake cleanup to report failure"),
        }
        drop(registered);
        assert!(block_on(operation.wait_settlement()).all_released());
    }

    #[test]
    fn test_rejected_cleanup_dispatch_preserves_resource_and_identity() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 1 }).unwrap();
        let accepting_port = Arc::new(FixedMimeExecutionPort::new(1, 1));
        accepting_port.prepare().unwrap();
        let worker = MimeExecutionWorker::new(accepting_port.clone());
        let owner = LocalFileSystem::host().unwrap().create_temp_file().unwrap();
        let staged = StagedMimeInput {
            owner: Mutex::new(StagedTempOwner::File { owner }),
            permits: Mutex::new(Vec::new()),
            worker,
            staged_bytes: AtomicU64::new(0),
            cleanup_id: Arc::new(ResourceIdentity::default()),
        };
        let mut operation = block_on(group.try_submit_async(Default::default(), move |scope| async move {
            let reservation = scope.reserve_resource().unwrap();
            commit_staged(reservation, staged)
        }))
        .unwrap();
        let registered = match block_on(operation.wait_outcome()).unwrap() {
            OperationOutcome::Succeeded(resource) => resource,
            _ => panic!("staged resource registration failed"),
        };
        let resource_id = registered.id();
        let cleanup_id = Arc::clone(&registered.cleanup_id);

        let owner = LocalFileSystem::host().unwrap().create_temp_file().unwrap();
        let retained_path = owner.path().to_path_buf();
        let rejecting_port = Arc::new(RejectingPort);
        rejecting_port.prepare().unwrap();
        rejecting_port.shutdown().unwrap();
        let rejecting_worker = MimeExecutionWorker::new(rejecting_port);
        let rejected_resource = StagedMimeInput {
            owner: Mutex::new(StagedTempOwner::File { owner }),
            permits: Mutex::new(Vec::new()),
            worker: rejecting_worker,
            staged_bytes: AtomicU64::new(0),
            cleanup_id: Arc::clone(&cleanup_id),
        };
        let cleanup = Arc::new(ManagedTempCleanup {
            resource_id: cleanup_id,
            attempts: AtomicUsize::new(0),
        });
        let outcome = block_on(cleanup.cleanup(
            rejected_resource,
            CleanupContext::new(ContextOptions::default()).unwrap(),
        ));

        match outcome {
            CleanupOutcome::Failed { failure, mut resource } => {
                assert_eq!(resource_id, failure.resource_id);
                assert_eq!(CleanupStage::Drop, failure.stage);
                assert!(failure.summary.to_string().contains("worker rejected cleanup job"));
                assert!(failure.source.is_some());
                assert!(retained_path.exists(), "rejected cleanup must retain the native file");
                resource
                    .owner
                    .get_mut()
                    .unwrap()
                    .cleanup()
                    .expect("test must explicitly remove the retained temp file");
            }
            _ => panic!("rejected worker dispatch must return the resource for recovery"),
        }

        drop(registered);
        assert!(block_on(operation.wait_settlement()).all_released());
        accepting_port.shutdown().unwrap();
    }

    #[test]
    fn test_successful_cleanup_removes_file_before_releasing_staging_budget() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 1 }).unwrap();
        let port = Arc::new(FixedMimeExecutionPort::new(1, 1));
        let services = MimeExecutionServices::new(manager, group.clone(), port.clone(), 16).unwrap();
        services.prepare().unwrap();
        let fixture_port = Arc::clone(&port);
        let mut identity_operation = block_on(group.try_submit_async(Default::default(), move |scope| async move {
            let owner = LocalFileSystem::host().unwrap().create_temp_file().unwrap();
            let resource = StagedMimeInput {
                owner: Mutex::new(StagedTempOwner::File { owner }),
                permits: Mutex::new(Vec::new()),
                worker: MimeExecutionWorker::new(fixture_port),
                staged_bytes: AtomicU64::new(0),
                cleanup_id: Arc::new(ResourceIdentity::default()),
            };
            commit_staged(scope.reserve_resource().unwrap(), resource)
        }))
        .unwrap();
        let registered = match block_on(identity_operation.wait_outcome()).unwrap() {
            OperationOutcome::Succeeded(resource) => resource,
            _ => panic!("identity fixture should register"),
        };
        let budget = services.staging_budget();
        let permit = budget.try_reserve(7).unwrap();
        let owner = LocalFileSystem::host().unwrap().create_temp_file().unwrap();
        let path = owner.path().to_path_buf();
        fs::write(&path, b"payload").unwrap();
        let resource = StagedMimeInput {
            owner: Mutex::new(StagedTempOwner::File { owner }),
            permits: Mutex::new(vec![permit]),
            worker: MimeExecutionWorker::new(port.clone()),
            staged_bytes: AtomicU64::new(0),
            cleanup_id: Arc::new(ResourceIdentity::published(registered.id())),
        };
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _owner = resource.owner.lock().unwrap();
                panic!("poison the owner mutex before cleanup recovery");
            }))
            .is_err()
        );
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _permits = resource.permits.lock().unwrap();
                panic!("poison the permit mutex before cleanup recovery");
            }))
            .is_err()
        );
        let cleanup = Arc::new(ManagedTempCleanup {
            resource_id: Arc::clone(&resource.cleanup_id),
            attempts: AtomicUsize::new(0),
        });

        assert_eq!(7, budget.used_bytes());
        assert!(path.exists());
        assert!(matches!(
            block_on(cleanup.cleanup(resource, CleanupContext::new(ContextOptions::default()).unwrap(),)),
            CleanupOutcome::Released
        ));
        assert!(!path.exists(), "cleanup releases the native file");
        assert_eq!(0, budget.used_bytes(), "permit release follows file cleanup");

        drop(registered);
        assert!(block_on(identity_operation.wait_settlement()).all_released());
        let shutdown = services.shutdown();
        let _ = block_on(shutdown.wait_services_settled()).unwrap();
        let _ = shutdown.stop_services().unwrap();
        let _report = block_on(shutdown.wait_group_shutdown()).unwrap();
    }

    #[test]
    fn test_sandbox_cleanup_failure_retains_resource_and_releases_on_retry() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 1 }).unwrap();
        let port = Arc::new(FixedMimeExecutionPort::new(1, 1));
        let services = Arc::new(MimeExecutionServices::new(manager, group.clone(), port.clone(), 16).unwrap());
        services.prepare().unwrap();
        let worker = MimeExecutionWorker::new(port.clone());
        let registered_cleanup_id = Arc::new(ResourceIdentity::default());
        let registered_worker = worker.clone();
        let registered_id = Arc::clone(&registered_cleanup_id);
        let mut operation = block_on(group.try_submit_async(Default::default(), move |scope| async move {
            let owner = LocalFileSystem::host().unwrap().create_temp_file().unwrap();
            commit_staged(
                scope.reserve_resource().unwrap(),
                StagedMimeInput {
                    owner: Mutex::new(StagedTempOwner::File { owner }),
                    permits: Mutex::new(Vec::new()),
                    worker: registered_worker,
                    staged_bytes: AtomicU64::new(0),
                    cleanup_id: registered_id,
                },
            )
        }))
        .unwrap();
        let registered = match block_on(operation.wait_outcome()).unwrap() {
            OperationOutcome::Succeeded(resource) => resource,
            _ => panic!("registered identity resource should succeed"),
        };

        let budget = services.staging_budget();
        let permit = budget.try_reserve(5).unwrap();
        let owner = LocalFileSystem::host().unwrap().create_temp_file().unwrap();
        let path = owner.path().to_path_buf();
        let sandbox = path.parent().unwrap().to_path_buf();
        let blocker = sandbox.join("cleanup-blocker");
        fs::write(&path, b"payload").unwrap();
        fs::write(&blocker, b"keep sandbox non-empty").unwrap();
        let resource = StagedMimeInput {
            owner: Mutex::new(StagedTempOwner::File { owner }),
            permits: Mutex::new(vec![permit]),
            worker,
            staged_bytes: AtomicU64::new(0),
            cleanup_id: Arc::new(ResourceIdentity::published(registered.id())),
        };
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _owner = resource.owner.lock().unwrap();
                panic!("poison the owner mutex before failed cleanup recovery");
            }))
            .is_err()
        );
        let cleanup = Arc::new(ManagedTempCleanup {
            resource_id: Arc::clone(&resource.cleanup_id),
            attempts: AtomicUsize::new(0),
        });

        match block_on(Arc::clone(&cleanup).cleanup(resource, CleanupContext::new(ContextOptions::default()).unwrap()))
        {
            CleanupOutcome::Failed { failure, resource } => {
                assert_eq!(registered.id(), failure.resource_id);
                assert!(failure.source.is_some());
                assert!(
                    !path.exists(),
                    "the staged file is removed before sandbox cleanup fails"
                );
                assert!(blocker.exists(), "the unrelated blocker keeps the sandbox recoverable");
                assert_eq!(
                    5,
                    budget.used_bytes(),
                    "failed cleanup retains the permit with its resource"
                );
                fs::remove_file(&blocker).unwrap();
                assert!(matches!(
                    block_on(
                        Arc::clone(&cleanup)
                            .cleanup(resource, CleanupContext::new(ContextOptions::default()).unwrap(),)
                    ),
                    CleanupOutcome::Released
                ));
            }
            _ => panic!("cleanup must report the retained non-empty sandbox"),
        }
        assert_eq!(0, budget.used_bytes(), "successful retry releases the permit");
        assert!(!sandbox.exists(), "successful retry removes the empty sandbox");

        drop(registered);
        assert!(block_on(operation.wait_settlement()).all_released());
        let shutdown = services.shutdown();
        let _ = block_on(shutdown.wait_services_settled()).unwrap();
        let _ = shutdown.stop_services().unwrap();
        let _report = block_on(shutdown.wait_group_shutdown()).unwrap();
    }

    #[test]
    fn test_transferred_staged_recovery_retries_after_service_shutdown() {
        let _recovery_port_guard = recovery_cleanup_test_guard();
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 1 }).unwrap();
        let port = Arc::new(FixedMimeExecutionPort::new(1, 1));
        let services = MimeExecutionServices::new(manager, group.clone(), port.clone(), 16).unwrap();
        services.prepare().unwrap();
        let budget = services.staging_budget();
        let permit = budget.try_reserve(7).unwrap();
        let owner = LocalFileSystem::host().unwrap().create_temp_file().unwrap();
        let path = owner.path().to_path_buf();
        let blocker = path.parent().unwrap().join("cleanup-blocker");
        fs::write(&path, b"payload").unwrap();
        fs::write(&blocker, b"keep cleanup directory non-empty").unwrap();
        let worker = MimeExecutionWorker::new(port.clone());
        let staged = StagedMimeInput {
            owner: Mutex::new(StagedTempOwner::File { owner }),
            permits: Mutex::new(vec![permit]),
            worker,
            staged_bytes: AtomicU64::new(7),
            cleanup_id: Arc::new(ResourceIdentity::default()),
        };
        let mut operation = block_on(group.try_submit_async(Default::default(), move |scope| async move {
            commit_staged(scope.reserve_resource().unwrap(), staged)
        }))
        .unwrap();
        let registered = match block_on(operation.wait_outcome()).unwrap() {
            OperationOutcome::Succeeded(resource) => resource,
            _ => panic!("staged resource registration should succeed"),
        };
        let resource_id = registered.id();
        drop(registered);
        let settlement = block_on(operation.wait_settlement());
        assert_eq!(
            1,
            settlement.recoveries.len(),
            "blocked directory retains cleanup recovery"
        );
        let recovery = settlement.recoveries[0].clone();
        assert_eq!(RecoveryPhase::Pending, recovery.snapshot().phase);
        assert!(
            !path.exists(),
            "the first cleanup removed the file but retained its non-empty directory"
        );
        assert!(blocker.exists(), "the blocker keeps the native cleanup recoverable");
        assert_eq!(7, budget.used_bytes(), "the failed owner retains its staging permit");

        let store = RecoveryStore::new(RecoveryStoreOptions::default()).unwrap();
        recovery.transfer_to(&store).unwrap();
        let failed_retry = store.try_retry(recovery.id()).unwrap().wait().unwrap();
        assert_eq!(RecoveryPhase::Pending, failed_retry.recovery.phase);
        assert_eq!(1, failed_retry.recovery.failures_total);
        let retry_failure = failed_retry.recovery.last_failures[0].summary.to_string();
        assert!(!retry_failure.is_empty(), "retry failure remains diagnosable");
        let shutdown = services.shutdown();
        assert!(block_on(shutdown.wait_services_settled()).unwrap().operations_settled);
        assert!(shutdown.stop_services().unwrap().services_stopped);
        fs::remove_file(&blocker).unwrap();

        let report = store.try_retry(recovery.id()).unwrap().wait().unwrap();
        assert_eq!(RecoveryPhase::Resolved, report.recovery.phase);
        assert!(
            report.recovery.last_failures.is_empty(),
            "confirmed cleanup clears failure history"
        );
        assert_eq!(0, report.recovery.failures_total);
        assert_eq!([resource_id], report.recovery.resources.as_ref());
        assert!(
            !path.parent().unwrap().exists(),
            "retry removes the retained directory after the blocker is removed"
        );
        assert_eq!(0, budget.used_bytes(), "successful recovery returns the staging permit");
        store.request_shutdown().wait().unwrap();
    }

    #[test]
    fn test_rejected_recovery_dispatch_retains_owner_identity_and_budget() {
        let _recovery_port_guard = recovery_cleanup_test_guard();
        let recovery_port = recovery_cleanup_port().unwrap();
        let started = Arc::new(std::sync::Barrier::new(2));
        let release = Arc::new(std::sync::Barrier::new(2));
        let worker_started = Arc::clone(&started);
        let worker_release = Arc::clone(&release);
        assert!(
            recovery_port
                .try_dispatch(Box::new(move || {
                    worker_started.wait();
                    worker_release.wait();
                }))
                .is_ok()
        );
        started.wait();
        let completed = Arc::new(AtomicUsize::new(0));
        for _ in 0..16 {
            let completed = Arc::clone(&completed);
            assert!(
                recovery_port
                    .try_dispatch(Box::new(move || {
                        completed.fetch_add(1, Ordering::Release);
                    }))
                    .is_ok()
            );
        }

        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 2 }).unwrap();
        let owner_port = Arc::new(FixedMimeExecutionPort::new(1, 1));
        let services = MimeExecutionServices::new(manager, group.clone(), owner_port.clone(), 16).unwrap();
        services.prepare().unwrap();
        let budget = services.staging_budget();
        let owner_budget = budget.clone();
        let owner_worker = MimeExecutionWorker::new(owner_port.clone());
        let mut identity_operation = block_on(group.try_submit_async(Default::default(), move |scope| async move {
            let owner = LocalFileSystem::host().unwrap().create_temp_file().unwrap();
            commit_staged(
                scope.reserve_resource().unwrap(),
                StagedMimeInput::new(owner, owner_budget.try_reserve(1).unwrap(), owner_worker),
            )
        }))
        .unwrap();
        let registered = match block_on(identity_operation.wait_outcome()).unwrap() {
            OperationOutcome::Succeeded(resource) => resource,
            _ => panic!("resource identity fixture should succeed"),
        };
        let resource_id = registered.id();
        let cleanup_id = Arc::clone(&registered.cleanup_id);

        let service_port = Arc::new(RejectingPort);
        service_port.prepare().unwrap();
        service_port.shutdown().unwrap();
        let permit = budget.try_reserve(5).unwrap();
        let owner = LocalFileSystem::host().unwrap().create_temp_file().unwrap();
        let path = owner.path().to_path_buf();
        let cleanup = Arc::new(ManagedTempCleanup {
            resource_id: Arc::clone(&cleanup_id),
            attempts: AtomicUsize::new(1),
        });
        let resource = StagedMimeInput {
            owner: Mutex::new(StagedTempOwner::File { owner }),
            permits: Mutex::new(vec![permit]),
            worker: MimeExecutionWorker::new(service_port),
            staged_bytes: AtomicU64::new(5),
            cleanup_id,
        };

        let resource = match block_on(
            Arc::clone(&cleanup).cleanup(resource, CleanupContext::new(ContextOptions::default()).unwrap()),
        ) {
            CleanupOutcome::Failed { failure, resource } => {
                assert_eq!(resource_id, failure.resource_id);
                assert!(failure.summary.to_string().contains("recovery cleanup port rejected"));
                assert!(path.exists());
                assert_eq!(6, budget.used_bytes());
                resource
            }
            _ => panic!("a full recovery queue must return the same owner"),
        };
        release.wait();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while completed.load(Ordering::Acquire) != 16 && std::time::Instant::now() < deadline {
            thread::yield_now();
        }
        assert_eq!(
            16,
            completed.load(Ordering::Acquire),
            "queued work must drain before retry"
        );

        assert!(matches!(
            block_on(cleanup.cleanup(resource, CleanupContext::new(ContextOptions::default()).unwrap())),
            CleanupOutcome::Released
        ));
        assert!(!path.exists());
        assert_eq!(
            1,
            budget.used_bytes(),
            "the separate registered identity fixture remains owned"
        );
        drop(registered);
        assert!(block_on(identity_operation.wait_settlement()).all_released());
        let shutdown = services.shutdown();
        let _ = block_on(shutdown.wait_services_settled()).unwrap();
        let _ = shutdown.stop_services().unwrap();
        let _ = block_on(shutdown.wait_group_shutdown()).unwrap();
    }
}
