// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Repeatable observer for MIME service shutdown and group drain.

use std::sync::atomic::Ordering;

use qubit_operation::ShutdownMode;
use qubit_operation::ShutdownReport;

use super::MimeExecutionServices;
use super::MimeShutdownSnapshot;

/// Repeatable MIME shutdown observer. Service settlement is independent of
/// caller outcome disposal; group shutdown can remain pending afterward.
#[derive(Clone)]
pub struct MimeShutdownObserver {
    /// Shared lifecycle and operation tracking state.
    pub(super) services: MimeExecutionServices,
}

impl MimeShutdownObserver {
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
    /// or transfer of every recovery handle. Returns
    /// [`crate::MimeError::OperationShutdownPending`] while any recovery is
    /// pending, running, or unconfirmed. Call from outside MIME operation
    /// bodies and async polls because the adapter may block waiting for its
    /// fixed workers to exit. This is idempotent.
    pub fn stop_services(&self) -> Result<MimeShutdownSnapshot, crate::MimeError> {
        if !self.services.operations_settled.load(Ordering::Acquire) {
            return Err(crate::MimeError::OperationShutdownPending);
        }
        let _lifecycle = self.services.lifecycle_lock.lock().unwrap_or_else(|e| e.into_inner());
        if self.services.lifecycle.load(Ordering::Acquire) != 3 {
            let _submissions = self.services.submission_guard();
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
            self.services.lifecycle.store(3, Ordering::Release);
        }
        Ok(self.snapshot())
    }

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
            services_stopped: self.services.lifecycle.load(Ordering::Acquire) == 3,
            unresolved_recoveries: self.services.unresolved_recovery_count(),
            group: handle.map(|value| value.snapshot()),
        }
    }

    /// Waits for group shutdown after workers and tracked operation settlements
    /// are complete. This may remain pending while callers retain typed
    /// handles.
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
    use std::future::Future;
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;
    use std::task::Context;
    use std::task::Poll;
    use std::task::Wake;
    use std::task::Waker;

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
    use crate::managed_temp_staging::ManagedStagingLimits;
    use crate::managed_temp_staging::stage_owned_in;

    struct CountingExecutionPort {
        worker_port: Arc<FixedMimeExecutionPort>,
        shutdown_calls: Arc<AtomicUsize>,
    }

    impl Debug for CountingExecutionPort {
        fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
            formatter.debug_struct("CountingExecutionPort").finish_non_exhaustive()
        }
    }

    struct ThreadWaker(std::thread::Thread);

    impl Wake for ThreadWaker {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }

        fn wake_by_ref(self: &Arc<Self>) {
            self.0.unpark();
        }
    }

    fn block_on<F: Future>(future: F) -> F::Output {
        let waker = Waker::from(Arc::new(ThreadWaker(std::thread::current())));
        let mut context = Context::from_waker(&waker);
        let mut future = Box::pin(future);
        loop {
            match future.as_mut().poll(&mut context) {
                Poll::Ready(output) => return output,
                Poll::Pending => std::thread::park(),
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
        std::fs::write(&blocker, b"keep the staging sandbox nonempty").unwrap();
        drop(resource);

        let settlement = block_on(operation.wait_settlement());
        assert_eq!(settlement.status, SettlementStatus::RecoveryRequired);
        assert_eq!(settlement.recoveries.len(), 1);

        let mut later_operation =
            block_on(group.try_submit_async(Default::default(), |_| async { Ok::<_, ()>(1) })).unwrap();
        services.track_observer(later_operation.observer());
        assert!(matches!(
            block_on(later_operation.wait_outcome()).unwrap(),
            OperationOutcome::Succeeded(1)
        ));
        block_on(later_operation.wait_settlement());

        let shutdown = services.shutdown();
        block_on(shutdown.wait_services_settled()).unwrap();
        assert_eq!(shutdown.snapshot().unresolved_recoveries, 1);
        let stopped = shutdown.stop_services();
        assert!(
            matches!(stopped, Err(MimeError::OperationShutdownPending)),
            "MIME workers must stay available while cleanup recovery is unresolved"
        );
        assert_eq!(shutdown_calls.load(Ordering::SeqCst), 0);

        std::fs::remove_file(blocker).unwrap();
        let recovery = settlement.recoveries[0].clone();
        let attempt = recovery.try_retry().unwrap();
        let retry = block_on(attempt.wait_async());
        assert!(retry.failures.is_empty());
        assert_eq!(recovery.snapshot().phase, RecoveryPhase::Resolved);
        assert_eq!(shutdown.snapshot().unresolved_recoveries, 0);

        shutdown.stop_services().unwrap();
        assert_eq!(shutdown_calls.load(Ordering::SeqCst), 1);
    }
}
