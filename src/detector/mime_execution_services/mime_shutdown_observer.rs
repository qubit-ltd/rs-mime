// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Repeatable observer for MIME service shutdown and group drain.

use std::sync::atomic::Ordering;

use qubit_operation::RecoveryHandle;
use qubit_operation::ShutdownMode;
use qubit_operation::ShutdownReport;

use super::MimeExecutionServices;
use super::MimeServiceLifecycle;
use super::MimeShutdownSnapshot;

/// Repeatable MIME shutdown observer. Service settlement is independent of
/// caller outcome disposal; group shutdown can remain pending afterward. The
/// observer is returned by [`MimeExecutionServices::shutdown`].
///
/// # Examples
///
/// ```
/// use qubit_mime::detector::mime_execution_services::MimeShutdownObserver;
///
/// fn operations_are_settled(observer: &MimeShutdownObserver) -> bool {
///     observer.snapshot().operations_settled
/// }
/// ```
#[must_use = "retain the observer to complete MIME service shutdown"]
#[derive(Clone)]
pub struct MimeShutdownObserver {
    /// Shared lifecycle and operation tracking state.
    pub(super) services: MimeExecutionServices,
}

impl MimeShutdownObserver {
    /// Returns service and group shutdown progress without waiting.
    ///
    /// # Returns
    /// A snapshot that includes the number of unresolved recovery handles.
    pub fn snapshot(&self) -> MimeShutdownSnapshot {
        let handle = self
            .services
            .group_shutdown
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        MimeShutdownSnapshot {
            operations_settled: self.services.operations_settled.load(Ordering::Acquire),
            services_stopped: self.services.lifecycle() == MimeServiceLifecycle::Stopped,
            unresolved_recoveries: self.services.unresolved_recovery_count(),
            group: handle.map(|value| value.snapshot()),
        }
    }

    /// Returns a repeatable snapshot of every unresolved recovery owned by
    /// the operation group. Callers can retry or transfer these handles even
    /// after their original typed operation handles have been dropped.
    #[must_use]
    pub fn recovery_handles_snapshot(&self) -> Vec<RecoveryHandle> {
        self.services.unresolved_recovery_handles()
    }

    /// Waits asynchronously until every tracked operation publishes a
    /// settlement report. Any recovery handles in those reports are retained
    /// for shutdown gating; callers must explicitly resolve or transfer them
    /// before [`Self::stop_services`] can stop the workers. This does not wait
    /// for caller-owned typed handles to consume or discard their outcomes.
    ///
    /// # Returns
    /// A snapshot showing operation settlement and any unresolved recoveries.
    ///
    /// # Errors
    /// Returns an error if waiting for an operation settlement fails.
    pub async fn wait_services_settled(&self) -> Result<MimeShutdownSnapshot, crate::MimeError> {
        loop {
            let pending = self
                .services
                .observers
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            for observer in pending {
                let report = observer.wait_settlement().await;
                self.services.track_recoveries(&report.recoveries);
            }

            let submissions = self.services.submission_guard();
            let unsettled = self
                .services
                .observers
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .any(|observer| observer.snapshot().settlement.is_none());
            if unsettled {
                drop(submissions);
                continue;
            }
            self.services.operations_settled.store(true, Ordering::Release);
            drop(submissions);
            return Ok(self.snapshot());
        }
    }

    /// Stops the MIME execution port after settlement and explicit resolution
    /// or transfer of every recovery handle. Call from outside MIME operation
    /// bodies and async polls because the adapter may block waiting for its
    /// fixed workers to exit. This is idempotent.
    ///
    /// # Returns
    /// A snapshot of service and operation-group shutdown progress.
    ///
    /// # Errors
    /// Returns [`crate::MimeError::OperationShutdownPending`] while a group
    /// operation remains active or any recovery is unresolved, or propagates
    /// an execution shutdown error.
    pub fn stop_services(&self) -> Result<MimeShutdownSnapshot, crate::MimeError> {
        if !self.services.operations_settled.load(Ordering::Acquire) {
            return Err(crate::MimeError::OperationShutdownPending);
        }
        let _lifecycle = self.services.lifecycle_lock.lock().unwrap_or_else(|e| e.into_inner());
        if self.services.lifecycle() != MimeServiceLifecycle::Stopped {
            let _submissions = self.services.submission_guard();
            // Freeze direct group submissions before taking the authoritative
            // recovery snapshot. Bound scopes from already-active operations
            // may still submit children until those operations finish.
            self.services.root_group.close_admission();
            if self.services.has_untracked_active_operations() {
                return Err(crate::MimeError::OperationShutdownPending);
            }
            if self.services.unresolved_recovery_count() != 0 {
                return Err(crate::MimeError::OperationShutdownPending);
            }
            self.services.execution.shutdown()?;
            let shutdown = self
                .services
                .root_group
                .request_shutdown(ShutdownMode::Drain)
                .as_async();
            *self.services.group_shutdown.lock().unwrap_or_else(|e| e.into_inner()) = Some(shutdown);
            self.services.lifecycle.store(MimeServiceLifecycle::Stopped as u8, Ordering::Release);
        }
        Ok(self.snapshot())
    }

    /// Waits for group shutdown after workers and tracked operation settlements
    /// are complete. This may remain pending while callers retain typed
    /// handles.
    ///
    /// # Returns
    /// The shutdown report after the operation group has drained.
    ///
    /// # Errors
    /// Returns [`crate::MimeError::OperationShutdownPending`] if services have
    /// not been stopped, or propagates an operation-group shutdown error.
    pub async fn wait_group_shutdown(&self) -> Result<ShutdownReport, crate::MimeError> {
        let handle = self
            .services
            .group_shutdown
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .ok_or(crate::MimeError::OperationShutdownPending)?;
        handle.wait().await.map_err(crate::MimeError::OperationShutdown)
    }
}

#[cfg(test)]
mod tests {
    use std::fmt::Debug;
    use std::fmt::Formatter;
    use std::fmt::Result as FmtResult;
    use std::fs;
    use std::future::Future;
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;
    use std::sync::mpsc;
    use std::task::Context;
    use std::task::Poll;
    use std::task::Wake;
    use std::task::Waker;
    use std::thread;
    use std::thread::Thread;

    use qubit_operation::GroupOptions;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOutcome;
    use qubit_operation::RecoveryPhase;
    use qubit_operation::SettlementStatus;

    use crate::MimeError;
    use crate::OwnedMimeInput;
    use crate::command_execution::FixedMimeExecutionPort;
    use crate::command_execution::MimeExecutionWorker;
    use crate::detector::InputExtent;
    use crate::detector::MimeExecutionPort;
    use crate::detector::MimeExecutionServices;
    use crate::detector::MimeServiceLifecycle;
    use crate::managed_temp_staging::ManagedStagingLimits;
    use crate::managed_temp_staging::stage_owned_in;

    /// Counts shutdown calls while forwarding execution operations to a worker
    /// port.
    struct CountingExecutionPort {
        worker_port: Arc<FixedMimeExecutionPort>,
        shutdown_calls: Arc<AtomicUsize>,
    }

    impl Debug for CountingExecutionPort {
        fn fmt(&self, formatter: &mut Formatter<'_>) -> FmtResult {
            formatter.debug_struct("CountingExecutionPort").finish_non_exhaustive()
        }
    }

    /// Wakes the thread parked while polling a test future.
    struct ThreadWaker(Thread);

    impl Wake for ThreadWaker {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }

        fn wake_by_ref(self: &Arc<Self>) {
            self.0.unpark();
        }
    }

    /// Polls a future synchronously, parking the current thread between
    /// wakeups.
    fn block_on<F: Future>(future: F) -> F::Output {
        let waker = Waker::from(Arc::new(ThreadWaker(thread::current())));
        let mut context = Context::from_waker(&waker);
        let mut future = Box::pin(future);
        loop {
            match future.as_mut().poll(&mut context) {
                Poll::Ready(output) => return output,
                Poll::Pending => thread::park(),
            }
        }
    }

    impl MimeExecutionPort for CountingExecutionPort {
        fn prepare(&self) -> Result<(), MimeError> {
            self.worker_port.prepare()
        }

        fn shutdown(&self) -> Result<(), MimeError> {
            self.shutdown_calls.fetch_add(1, Ordering::SeqCst);
            self.worker_port.shutdown()
        }
    }

    #[test]
    fn test_shutdown_observer_reports_empty_service_lifecycle_and_is_idempotent() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 1 }).unwrap();
        let worker_port = Arc::new(FixedMimeExecutionPort::new(1, 1));
        let shutdown_calls = Arc::new(AtomicUsize::new(0));
        let execution = Arc::new(CountingExecutionPort {
            worker_port,
            shutdown_calls: Arc::clone(&shutdown_calls),
        });
        let services = MimeExecutionServices::new(manager, group, execution, 16).unwrap();
        let group_shutdown = Arc::clone(&services.group_shutdown);
        assert!(
            thread::spawn(move || {
                let _guard = group_shutdown.lock().unwrap();
                panic!("poison group shutdown observer lock");
            })
            .join()
            .is_err()
        );
        let shutdown = services.shutdown();
        let cloned_observer = shutdown.clone();

        assert!(!shutdown.snapshot().operations_settled);
        assert!(!shutdown.snapshot().services_stopped);
        assert!(matches!(
            block_on(shutdown.wait_group_shutdown()),
            Err(MimeError::OperationShutdownPending)
        ));
        assert!(matches!(
            shutdown.stop_services(),
            Err(MimeError::OperationShutdownPending)
        ));

        let settled = block_on(shutdown.wait_services_settled()).unwrap();
        assert!(settled.operations_settled);
        assert!(!settled.services_stopped);
        assert!(settled.group.is_none());

        let stopped = shutdown.stop_services().unwrap();
        assert!(stopped.services_stopped);
        assert!(stopped.group.is_some());
        assert_eq!(1, shutdown_calls.load(Ordering::SeqCst));
        let _ = shutdown.stop_services().unwrap();
        assert_eq!(1, shutdown_calls.load(Ordering::SeqCst));
        assert!(cloned_observer.snapshot().services_stopped);
        assert!(cloned_observer.stop_services().unwrap().services_stopped);

        let report = block_on(cloned_observer.wait_group_shutdown()).unwrap();
        assert_eq!(0, report.snapshot.active_operations);
        assert_eq!(0, report.snapshot.queued_operations);
        assert_eq!(0, report.snapshot.cleaning_resources);
    }

    #[test]
    fn test_services_prepare_and_child_acceptance_follow_shutdown_lifecycle() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 1 }).unwrap();
        let worker_port = Arc::new(FixedMimeExecutionPort::new(1, 1));
        let shutdown_calls = Arc::new(AtomicUsize::new(0));
        let execution = Arc::new(CountingExecutionPort {
            worker_port,
            shutdown_calls: Arc::clone(&shutdown_calls),
        });
        let services = MimeExecutionServices::new(manager, group, execution, 16).unwrap();

        assert!(!services.is_prepared());
        assert!(!services.accepts_children());
        assert_eq!(MimeServiceLifecycle::NotStarted, services.lifecycle());
        services.prepare().unwrap();
        assert!(services.is_prepared());
        assert_eq!(MimeServiceLifecycle::Running, services.lifecycle());
        assert!(services.accepts_children());
        services.prepare().unwrap();
        assert!(services.is_prepared());

        let shutdown = services.shutdown();
        assert_eq!(MimeServiceLifecycle::Draining, services.lifecycle());
        assert!(!services.is_prepared());
        assert!(services.accepts_children());
        let _ = block_on(shutdown.wait_services_settled()).unwrap();
        assert!(!services.accepts_children());
        let _ = shutdown.stop_services().unwrap();
        assert!(!services.is_prepared());
        assert!(!services.accepts_children());
        assert_eq!(MimeServiceLifecycle::Stopped, services.lifecycle());
        assert_eq!(1, shutdown_calls.load(Ordering::SeqCst));
    }

    #[test]
    fn test_shutdown_before_prepare_has_a_distinct_closed_lifecycle() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 1 }).unwrap();
        let execution = Arc::new(CountingExecutionPort {
            worker_port: Arc::new(FixedMimeExecutionPort::new(1, 1)),
            shutdown_calls: Arc::new(AtomicUsize::new(0)),
        });
        let services = MimeExecutionServices::new(manager, group, execution, 16).unwrap();

        let shutdown = services.shutdown();
        assert_eq!(MimeServiceLifecycle::ClosedWithoutStart, services.lifecycle());
        assert!(!services.accepts_children());
        assert!(services.prepare().is_err());
        let _ = block_on(shutdown.wait_services_settled()).unwrap();
        let _ = shutdown.stop_services().unwrap();
        assert_eq!(MimeServiceLifecycle::Stopped, services.lifecycle());
    }

    #[test]
    fn test_wait_services_settled_remains_pending_until_tracked_operation_finishes() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 1 }).unwrap();
        let worker_port = Arc::new(FixedMimeExecutionPort::new(1, 1));
        let shutdown_calls = Arc::new(AtomicUsize::new(0));
        let execution = Arc::new(CountingExecutionPort {
            worker_port,
            shutdown_calls: Arc::clone(&shutdown_calls),
        });
        let services = MimeExecutionServices::new(manager, group.clone(), execution, 16).unwrap();
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let operation = block_on(group.try_submit_async(Default::default(), move |_| async move {
            started_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            Ok::<_, ()>(())
        }))
        .unwrap();
        services.track_observer(operation.observer());
        started_rx.recv().expect("operation should enter its blocked body");

        let shutdown = services.shutdown();
        let mut settled_future = Box::pin(shutdown.wait_services_settled());
        let waker = Waker::from(Arc::new(ThreadWaker(thread::current())));
        let mut context = Context::from_waker(&waker);
        assert!(matches!(settled_future.as_mut().poll(&mut context), Poll::Pending));
        assert!(!shutdown.snapshot().operations_settled);

        release_tx.send(()).unwrap();
        let settled = block_on(settled_future).unwrap();
        assert!(settled.operations_settled);
        // Keep the typed outcome in the caller-owned handle: settlement can
        // complete while group drain still waits for the outcome to be released.
        let _ = shutdown.stop_services().unwrap();
        let mut group_shutdown = Box::pin(shutdown.wait_group_shutdown());
        let waker = Waker::from(Arc::new(ThreadWaker(thread::current())));
        let mut context = Context::from_waker(&waker);
        assert!(matches!(group_shutdown.as_mut().poll(&mut context), Poll::Pending));
        drop(operation);
        let group_report = block_on(group_shutdown).unwrap();
        assert_eq!(0, group_report.snapshot.active_operations);
        assert_eq!(0, group_report.snapshot.queued_operations);
        assert_eq!(1, shutdown_calls.load(Ordering::SeqCst));
    }

    #[test]
    fn test_shutdown_gate_waits_for_direct_group_operation_before_stopping_workers() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 1 }).unwrap();
        let shutdown_calls = Arc::new(AtomicUsize::new(0));
        let services = MimeExecutionServices::new(
            manager,
            group.clone(),
            Arc::new(CountingExecutionPort {
                worker_port: Arc::new(FixedMimeExecutionPort::new(1, 1)),
                shutdown_calls: Arc::clone(&shutdown_calls),
            }),
            16,
        )
        .unwrap();
        services.prepare().unwrap();
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let mut operation = block_on(group.try_submit_async(Default::default(), move |_| async move {
            started_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            Ok::<_, ()>(())
        }))
        .unwrap();
        started_rx.recv().unwrap();

        let shutdown = services.shutdown();
        assert!(block_on(shutdown.wait_services_settled()).unwrap().operations_settled);
        assert!(services.accepts_children());
        assert!(matches!(
            shutdown.stop_services(),
            Err(MimeError::OperationShutdownPending)
        ));
        assert_eq!(0, shutdown_calls.load(Ordering::SeqCst));

        release_tx.send(()).unwrap();
        let _ = block_on(operation.wait_outcome()).unwrap();
        block_on(operation.wait_settlement());
        assert!(shutdown.stop_services().unwrap().services_stopped);
        assert_eq!(1, shutdown_calls.load(Ordering::SeqCst));
    }

    #[test]
    fn test_settlement_waits_for_all_tracked_operations_and_group_drain_releases_handles() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 2 }).unwrap();
        let worker_port = Arc::new(FixedMimeExecutionPort::new(1, 2));
        let shutdown_calls = Arc::new(AtomicUsize::new(0));
        let execution = Arc::new(CountingExecutionPort {
            worker_port,
            shutdown_calls: Arc::clone(&shutdown_calls),
        });
        let services = MimeExecutionServices::new(manager, group.clone(), execution, 16).unwrap();

        let mut completed =
            block_on(group.try_submit_async(Default::default(), |_| async { Ok::<_, ()>("done") })).unwrap();
        services.track_observer(completed.observer());
        assert!(matches!(
            block_on(completed.wait_outcome()).unwrap(),
            OperationOutcome::Succeeded("done")
        ));
        block_on(completed.wait_settlement());

        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let pending = block_on(group.try_submit_async(Default::default(), move |_| async move {
            started_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            Ok::<_, ()>("released")
        }))
        .unwrap();
        services.track_observer(pending.observer());
        started_rx.recv().expect("second operation should start");

        let shutdown = services.shutdown();
        let mut settled_future = Box::pin(shutdown.wait_services_settled());
        let waker = Waker::from(Arc::new(ThreadWaker(thread::current())));
        let mut context = Context::from_waker(&waker);
        assert!(matches!(settled_future.as_mut().poll(&mut context), Poll::Pending));
        assert!(!shutdown.snapshot().operations_settled);

        release_tx.send(()).unwrap();
        assert!(block_on(settled_future).unwrap().operations_settled);
        let _ = shutdown.stop_services().unwrap();

        let mut group_shutdown = Box::pin(shutdown.wait_group_shutdown());
        let waker = Waker::from(Arc::new(ThreadWaker(thread::current())));
        let mut context = Context::from_waker(&waker);
        assert!(matches!(group_shutdown.as_mut().poll(&mut context), Poll::Pending));
        drop(completed);
        drop(pending);
        let report = block_on(group_shutdown).unwrap();
        assert_eq!(0, report.snapshot.active_operations);
        assert_eq!(0, report.snapshot.queued_operations);
        assert_eq!(1, shutdown_calls.load(Ordering::SeqCst));
    }

    #[cfg(unix)]
    #[test]
    fn test_shutdown_harvests_recovery_from_child_dispatched_after_first_stop_attempt() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 2 }).unwrap();
        let worker_port = Arc::new(FixedMimeExecutionPort::new(1, 2));
        let shutdown_calls = Arc::new(AtomicUsize::new(0));
        let execution = Arc::new(CountingExecutionPort {
            worker_port: Arc::clone(&worker_port),
            shutdown_calls: Arc::clone(&shutdown_calls),
        });
        let services = MimeExecutionServices::new(manager, group.clone(), execution, 1024).unwrap();
        services.prepare().unwrap();

        let (parent_started_tx, parent_started_rx) = mpsc::channel();
        let (release_parent_tx, release_parent_rx) = mpsc::channel();
        let (blocker_tx, blocker_rx) = mpsc::channel();
        let child_limits = ManagedStagingLimits {
            max_bytes: 128,
            budget: services.staging_budget(),
            worker: MimeExecutionWorker::new(worker_port),
        };
        let child_input = OwnedMimeInput::bytes(
            Arc::from(b"direct child recovery after drain".as_slice()),
            InputExtent::Complete,
        );
        let child_read_tree = services.new_read_tree(128);
        let mut parent = block_on(group.try_submit_async(Default::default(), move |scope| async move {
            parent_started_tx.send(()).unwrap();
            release_parent_rx.recv().unwrap();

            let child = scope
                .manager()
                .as_async()
                .try_submit(Default::default(), move |child_scope| async move {
                    stage_owned_in(&child_scope, child_input, child_limits, child_read_tree).await
                })
                .unwrap();
            let mut child = child;
            let resource = match child.wait_outcome().await.unwrap() {
                OperationOutcome::Succeeded(resource) => resource,
                _ => panic!("staging child should succeed before cleanup"),
            };
            let path = resource.path().unwrap();
            let sandbox = path.parent().unwrap();
            let blocker = sandbox.join("post-drain-child-recovery-blocker");
            fs::write(&blocker, b"retain the child staging sandbox").unwrap();
            drop(resource);
            let _settlement = child.wait_settlement().await;
            drop(child);
            blocker_tx.send(blocker).unwrap();
            Ok::<_, ()>(())
        }))
        .unwrap();
        parent_started_rx
            .recv()
            .expect("direct parent should enter its barrier");

        let shutdown = services.shutdown();
        assert!(block_on(shutdown.wait_services_settled()).unwrap().operations_settled);
        assert!(matches!(
            shutdown.stop_services(),
            Err(MimeError::OperationShutdownPending)
        ));
        assert_eq!(0, shutdown_calls.load(Ordering::SeqCst));

        release_parent_tx.send(()).unwrap();
        assert!(matches!(
            block_on(parent.wait_outcome()).unwrap(),
            OperationOutcome::Succeeded(())
        ));
        let parent_settlement = block_on(parent.wait_settlement());
        assert!(parent_settlement.recoveries.is_empty());
        drop(parent);
        let blocker = blocker_rx
            .recv()
            .expect("parent should publish its child cleanup blocker");

        assert_eq!(1, shutdown.snapshot().unresolved_recoveries);
        let recoveries = shutdown.recovery_handles_snapshot();
        assert_eq!(1, recoveries.len());
        assert_eq!(recoveries[0].id(), shutdown.recovery_handles_snapshot()[0].id());
        assert!(matches!(
            shutdown.stop_services(),
            Err(MimeError::OperationShutdownPending)
        ));
        assert_eq!(0, shutdown_calls.load(Ordering::SeqCst));

        fs::remove_file(blocker).unwrap();
        let recovery = recoveries[0].clone();
        let attempt = recovery.try_retry().unwrap();
        let retry = block_on(attempt.wait_async());
        assert!(retry.failures.is_empty());
        assert_eq!(RecoveryPhase::Resolved, recovery.snapshot().phase);
        assert_eq!(0, shutdown.snapshot().unresolved_recoveries);

        assert!(shutdown.stop_services().unwrap().services_stopped);
        assert_eq!(1, shutdown_calls.load(Ordering::SeqCst));
        let report = block_on(shutdown.wait_group_shutdown()).unwrap();
        assert_eq!(0, report.snapshot.active_operations);
        assert_eq!(0, report.snapshot.queued_operations);
    }

    #[cfg(unix)]
    #[test]
    fn test_shutdown_waits_for_explicit_recovery_before_stopping_workers() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 4 }).unwrap();
        let worker_port = Arc::new(FixedMimeExecutionPort::new(1, 4));
        let shutdown_calls = Arc::new(AtomicUsize::new(0));
        let execution = Arc::new(CountingExecutionPort {
            worker_port: Arc::clone(&worker_port),
            shutdown_calls: Arc::clone(&shutdown_calls),
        });
        let services = MimeExecutionServices::new(manager, group.clone(), execution, 1024).unwrap();
        services.prepare().unwrap();
        let limits = ManagedStagingLimits {
            max_bytes: 128,
            budget: services.staging_budget(),
            worker: MimeExecutionWorker::new(worker_port),
        };
        let input = OwnedMimeInput::bytes(Arc::from(b"retry shutdown cleanup".as_slice()), InputExtent::Complete);
        let read_tree = services.new_read_tree(128);
        let body_read_tree = Arc::clone(&read_tree);
        let mut operation = block_on(group.try_submit_async(Default::default(), move |scope| async move {
            stage_owned_in(&scope, input, limits, body_read_tree).await
        }))
        .unwrap();
        services.track_observer(operation.observer());
        let resource = match block_on(operation.wait_outcome()).unwrap() {
            OperationOutcome::Succeeded(resource) => resource,
            _ => panic!("staging operation did not succeed"),
        };
        let path = resource.path().unwrap();
        let sandbox = path.parent().unwrap().to_path_buf();
        let blocker = sandbox.join("shutdown-cleanup-blocker");
        fs::write(&blocker, b"keep the staging sandbox nonempty").unwrap();
        drop(resource);

        let settlement = block_on(operation.wait_settlement());
        assert_eq!(settlement.status, SettlementStatus::RecoveryRequired);
        assert_eq!(settlement.recoveries.len(), 1);
        assert!(super::super::is_unresolved_recovery(&settlement.recoveries[0]));

        let mut later_operation =
            block_on(group.try_submit_async(Default::default(), |_| async { Ok::<_, ()>(1) })).unwrap();
        services.track_observer(later_operation.observer());
        assert!(matches!(
            block_on(later_operation.wait_outcome()).unwrap(),
            OperationOutcome::Succeeded(1)
        ));
        block_on(later_operation.wait_settlement());

        let shutdown = services.shutdown();
        let _ = block_on(shutdown.wait_services_settled()).unwrap();
        assert_eq!(shutdown.snapshot().unresolved_recoveries, 1);
        let handles = shutdown.recovery_handles_snapshot();
        assert_eq!(1, handles.len(), "group ledger includes the recovery owner");
        assert_eq!(settlement.recoveries[0].id(), handles[0].id());
        drop(operation);
        assert_eq!(handles[0].id(), shutdown.recovery_handles_snapshot()[0].id());
        services.track_recoveries(&settlement.recoveries);
        assert_eq!(
            1,
            shutdown.snapshot().unresolved_recoveries,
            "tracking the same recovery twice must keep one shutdown blocker"
        );
        let stopped = shutdown.stop_services();
        assert!(
            matches!(stopped, Err(MimeError::OperationShutdownPending)),
            "MIME workers must stay available while cleanup recovery is unresolved"
        );
        assert_eq!(shutdown_calls.load(Ordering::SeqCst), 0);

        fs::remove_file(blocker).unwrap();
        let recovery = settlement.recoveries[0].clone();
        let attempt = recovery.try_retry().unwrap();
        let retry = block_on(attempt.wait_async());
        assert!(retry.failures.is_empty());
        assert_eq!(recovery.snapshot().phase, RecoveryPhase::Resolved);
        assert!(!super::super::is_unresolved_recovery(&recovery));
        assert_eq!(shutdown.snapshot().unresolved_recoveries, 0);

        let _ = shutdown.stop_services().unwrap();
        assert_eq!(shutdown_calls.load(Ordering::SeqCst), 1);
    }
}
