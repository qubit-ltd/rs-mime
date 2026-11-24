// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Repeatable observer for MIME service shutdown and group drain.

use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use qubit_operation::RecoveryHandle;
use qubit_operation::ShutdownMode;
use qubit_operation::ShutdownReport;

use super::MimeExecutionServices;
use super::MimeServiceLifecycle;
use super::MimeShutdownSnapshot;

/// Releases the single stop execution claim on every exit, including panic.
struct StopPermit<'a>(&'a AtomicBool);

impl Drop for StopPermit<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

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
            self.services
                .operations_settled
                .store(true, Ordering::Release);
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
    /// Returns [`crate::MimeError::OperationShutdownPending`] while the group
    /// has unsettled operation scopes or any recovery is unresolved, or
    /// propagates an execution shutdown error.
    pub fn stop_services(&self) -> Result<MimeShutdownSnapshot, crate::MimeError> {
        let _stop;
        {
            let _lifecycle = self
                .services
                .lifecycle_lock
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if self.services.lifecycle() == MimeServiceLifecycle::Stopped {
                return Ok(self.snapshot());
            }
            if self.services.stopping.swap(true, Ordering::AcqRel) {
                return Err(crate::MimeError::OperationShutdownPending);
            }
            _stop = StopPermit(&self.services.stopping);
            if self.services.preparing.load(Ordering::Acquire) {
                return Err(crate::MimeError::OperationShutdownPending);
            }
            let _submissions = self.services.submission_guard();
            // Freeze MIME-mediated submissions only once. Accepted direct
            // scope descendants remain covered by the group settlement gate.
            if !self.services.admission_frozen.swap(true, Ordering::AcqRel) {
                self.services.root_group.close_admission();
            }
            if !self.services.operations_settled.load(Ordering::Acquire) {
                return Err(crate::MimeError::OperationShutdownPending);
            }
            if self.services.has_unsettled_operations() {
                return Err(crate::MimeError::OperationShutdownPending);
            }
            if self.services.unresolved_recovery_count() != 0 {
                return Err(crate::MimeError::OperationShutdownPending);
            }
        }
        self.services.execution.shutdown()?;
        let _lifecycle = self
            .services
            .lifecycle_lock
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let shutdown = self
            .services
            .root_group
            .request_shutdown(ShutdownMode::Drain)
            .as_async();
        *self
            .services
            .group_shutdown
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(shutdown);
        self.services
            .lifecycle
            .store(MimeServiceLifecycle::Stopped as u8, Ordering::Release);
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
        handle
            .wait()
            .await
            .map_err(crate::MimeError::OperationShutdown)
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
    use std::sync::Mutex;
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;
    use std::sync::mpsc;
    use std::task::Context;
    use std::task::Poll;
    use std::task::Wake;
    use std::task::Waker;
    use std::thread;
    use std::thread::Thread;

    use qubit_local_files::LocalFileSystem;
    use qubit_local_files::options::LocalTempFileOptions;
    use qubit_local_files::policy::LocalPrivacyRequirement;
    use qubit_operation::AdmissionError;
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
    use crate::managed_temp_staging::create_empty_staged_with;
    use crate::managed_temp_staging::stage_owned_in;

    /// Counts shutdown calls while forwarding execution operations to a worker
    /// port.
    struct CountingExecutionPort {
        worker_port: Arc<FixedMimeExecutionPort>,
        shutdown_calls: Arc<AtomicUsize>,
    }

    /// Runs a deterministic submission attempt just before stopping the worker.
    struct LateSubmissionExecutionPort {
        worker_port: Arc<FixedMimeExecutionPort>,
        shutdown_calls: Arc<AtomicUsize>,
        before_shutdown: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    }

    struct SignalShutdownExecutionPort {
        worker_port: Arc<FixedMimeExecutionPort>,
        shutdown_started: Mutex<Option<mpsc::Sender<()>>>,
    }

    struct ControlledPrepareExecutionPort {
        worker_port: Arc<FixedMimeExecutionPort>,
        during_prepare: Mutex<Option<Box<dyn FnOnce() + Send>>>,
        shutdown_calls: Arc<AtomicUsize>,
        prepare_error: AtomicBool,
    }

    struct ReleaseOnDrop(mpsc::Sender<()>);

    impl Drop for ReleaseOnDrop {
        fn drop(&mut self) {
            let _ = self.0.send(());
        }
    }

    impl Debug for LateSubmissionExecutionPort {
        fn fmt(&self, formatter: &mut Formatter<'_>) -> FmtResult {
            formatter
                .debug_struct("LateSubmissionExecutionPort")
                .finish_non_exhaustive()
        }
    }

    impl MimeExecutionPort for LateSubmissionExecutionPort {
        fn prepare(&self) -> Result<(), MimeError> {
            self.worker_port.prepare()
        }

        fn shutdown(&self) -> Result<(), MimeError> {
            if let Some(before_shutdown) = self
                .before_shutdown
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .take()
            {
                before_shutdown();
            }
            self.shutdown_calls.fetch_add(1, Ordering::SeqCst);
            self.worker_port.shutdown()
        }
    }

    impl Debug for SignalShutdownExecutionPort {
        fn fmt(&self, formatter: &mut Formatter<'_>) -> FmtResult {
            formatter
                .debug_struct("SignalShutdownExecutionPort")
                .finish_non_exhaustive()
        }
    }

    impl MimeExecutionPort for SignalShutdownExecutionPort {
        fn prepare(&self) -> Result<(), MimeError> {
            self.worker_port.prepare()
        }

        fn shutdown(&self) -> Result<(), MimeError> {
            if let Some(sender) = self
                .shutdown_started
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take()
            {
                sender.send(()).unwrap();
            }
            self.worker_port.shutdown()
        }
    }

    impl Debug for ControlledPrepareExecutionPort {
        fn fmt(&self, formatter: &mut Formatter<'_>) -> FmtResult {
            formatter
                .debug_struct("ControlledPrepareExecutionPort")
                .finish_non_exhaustive()
        }
    }

    impl MimeExecutionPort for ControlledPrepareExecutionPort {
        fn prepare(&self) -> Result<(), MimeError> {
            self.worker_port.prepare()?;
            let callback = self.during_prepare.lock().unwrap().take();
            if let Some(callback) = callback {
                callback();
            }
            if self.prepare_error.swap(false, Ordering::SeqCst) {
                return Err(MimeError::ExecutionServicesRequired);
            }
            Ok(())
        }

        fn shutdown(&self) -> Result<(), MimeError> {
            self.shutdown_calls.fetch_add(1, Ordering::SeqCst);
            self.worker_port.shutdown()
        }
    }

    impl Debug for CountingExecutionPort {
        fn fmt(&self, formatter: &mut Formatter<'_>) -> FmtResult {
            formatter
                .debug_struct("CountingExecutionPort")
                .finish_non_exhaustive()
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
    fn test_reentrant_stop_during_execution_shutdown_returns_pending() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 1 }).unwrap();
        let worker_port = Arc::new(FixedMimeExecutionPort::new(1, 1));
        let shutdown_calls = Arc::new(AtomicUsize::new(0));
        let execution = Arc::new(LateSubmissionExecutionPort {
            worker_port,
            shutdown_calls: Arc::clone(&shutdown_calls),
            before_shutdown: Mutex::new(None),
        });
        let services = MimeExecutionServices::new(manager, group, execution.clone(), 16).unwrap();
        services.prepare().unwrap();
        let observer = services.shutdown();
        let _ = block_on(observer.wait_services_settled()).unwrap();
        let runtime = crate::detector::MimeRuntime::builtin()
            .unwrap()
            .with_execution_services(services.clone());
        let original: Arc<[u8]> = Arc::from(b"shutdown callback input".as_slice());
        let callback_original = Arc::clone(&original);
        let (result_tx, result_rx) = mpsc::channel();
        let reentrant_observer = observer.clone();
        let callback_runtime = runtime.clone();
        *execution.before_shutdown.lock().unwrap() = Some(Box::new(move || {
            let failure = match callback_runtime.try_submit_detection(
                OwnedMimeInput::bytes(
                    Arc::clone(&callback_original),
                    crate::detector::InputExtent::Complete,
                ),
                Default::default(),
            ) {
                Err(failure) => failure,
                Ok(_) => panic!("shutdown callback reentry must return its request"),
            };
            let input_returned = match failure.input {
                crate::OwnedMimeInput::Bytes { content, .. } => {
                    Arc::ptr_eq(&content, &callback_original)
                }
                crate::OwnedMimeInput::Provider { .. } => false,
            };
            let reentrant_stop = reentrant_observer.stop_services();
            result_tx
                .send((failure.reason, input_returned, reentrant_stop))
                .unwrap();
        }));

        let stopping_observer = observer.clone();
        let (stop_tx, stop_rx) = mpsc::channel();
        thread::spawn(move || stop_tx.send(stopping_observer.stop_services()).unwrap());
        assert!(
            stop_rx
                .recv_timeout(std::time::Duration::from_secs(2))
                .expect("shutdown must return after a reentrant request is rejected")
                .unwrap()
                .services_stopped
        );
        let (reason, input_returned, reentrant_stop) = result_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap();
        assert!(matches!(
            reason,
            crate::detector::MimeSubmitRejection::ExecutionServicesRequired
                | crate::detector::MimeSubmitRejection::Admission(_)
        ));
        assert!(input_returned);
        assert!(matches!(
            reentrant_stop,
            Err(MimeError::OperationShutdownPending)
        ));
        assert_eq!(1, shutdown_calls.load(Ordering::SeqCst));
        assert_eq!(original.as_ref(), b"shutdown callback input");
    }

    #[test]
    fn test_accepted_worker_job_reentrant_mime_submit_returns_original_input() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 1 }).unwrap();
        let worker_port = Arc::new(FixedMimeExecutionPort::new(1, 2));
        let (shutdown_started_tx, shutdown_started_rx) = mpsc::channel();
        let execution = Arc::new(SignalShutdownExecutionPort {
            worker_port: Arc::clone(&worker_port),
            shutdown_started: Mutex::new(Some(shutdown_started_tx)),
        });
        let services = MimeExecutionServices::new(manager, group, execution, 1024).unwrap();
        services.prepare().unwrap();
        let runtime = crate::detector::MimeRuntime::builtin()
            .unwrap()
            .with_execution_services(services.clone());
        let (result_tx, result_rx) = mpsc::channel();
        let original: Arc<[u8]> = Arc::from(b"accepted worker job".as_slice());
        let job_runtime = runtime.clone();
        let job_original = Arc::clone(&original);
        let accepted = worker_port.try_dispatch(Box::new(move || {
            shutdown_started_rx.recv().unwrap();
            let failure = match job_runtime.try_submit_detection(
                OwnedMimeInput::bytes(Arc::clone(&job_original), InputExtent::Complete),
                Default::default(),
            ) {
                Err(failure) => failure,
                Ok(_) => panic!("shutdown must reject MIME reentry from an accepted worker job"),
            };
            let returned = match failure.input {
                crate::OwnedMimeInput::Bytes { content, .. } => {
                    Arc::ptr_eq(&content, &job_original)
                }
                crate::OwnedMimeInput::Provider { .. } => false,
            };
            result_tx.send((failure.reason, returned)).unwrap();
        }));
        assert!(
            accepted.is_ok(),
            "worker job should be accepted before shutdown"
        );
        let observer = services.shutdown();
        let _ = block_on(observer.wait_services_settled()).unwrap();

        let stopping = observer.clone();
        let stop_thread = thread::spawn(move || stopping.stop_services());
        let (reason, returned) = result_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        assert!(matches!(
            reason,
            crate::detector::MimeSubmitRejection::ExecutionServicesRequired
                | crate::detector::MimeSubmitRejection::Admission(_)
        ));
        assert!(
            returned,
            "the rejected request must return the same Arc input"
        );
        assert!(stop_thread.join().unwrap().unwrap().services_stopped);
        assert_eq!(original.as_ref(), b"accepted worker job");
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
        assert!(services.accepts_children());
        let _ = shutdown.stop_services().unwrap();
        assert!(!services.is_prepared());
        assert!(!services.accepts_children());
        assert_eq!(MimeServiceLifecycle::Stopped, services.lifecycle());
        assert_eq!(1, shutdown_calls.load(Ordering::SeqCst));
    }

    #[test]
    fn test_prepare_callback_can_shutdown_without_publishing_running_or_stopping_early() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 1 }).unwrap();
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let shutdown_calls = Arc::new(AtomicUsize::new(0));
        let execution = Arc::new(ControlledPrepareExecutionPort {
            worker_port: Arc::new(FixedMimeExecutionPort::new(1, 1)),
            during_prepare: Mutex::new(None),
            shutdown_calls: Arc::clone(&shutdown_calls),
            prepare_error: AtomicBool::new(false),
        });
        let services = MimeExecutionServices::new(
            manager,
            group,
            Arc::clone(&execution) as Arc<dyn MimeExecutionPort>,
            16,
        )
        .unwrap();
        let callback_services = services.clone();
        *execution.during_prepare.lock().unwrap() = Some(Box::new(move || {
            let observer = callback_services.shutdown();
            assert_eq!(
                MimeServiceLifecycle::Draining,
                callback_services.lifecycle()
            );
            entered_tx.send(observer).unwrap();
            release_rx.recv().unwrap();
        }));
        let preparing_services = services.clone();
        let prepare_thread = thread::spawn(move || preparing_services.prepare());
        let shutdown = entered_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();

        assert_eq!(MimeServiceLifecycle::Draining, services.lifecycle());
        assert!(!services.accepts_children());
        assert!(matches!(
            shutdown.stop_services(),
            Err(MimeError::OperationShutdownPending)
        ));
        assert_eq!(0, shutdown_calls.load(Ordering::SeqCst));

        release_tx.send(()).unwrap();
        prepare_thread.join().unwrap().unwrap();
        assert_eq!(MimeServiceLifecycle::Draining, services.lifecycle());
        assert!(!services.is_prepared());
        assert!(matches!(
            shutdown.stop_services(),
            Err(MimeError::OperationShutdownPending)
        ));
        let _ = block_on(shutdown.wait_services_settled()).unwrap();
        assert!(shutdown.stop_services().unwrap().services_stopped);
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
        assert_eq!(
            MimeServiceLifecycle::ClosedWithoutStart,
            services.lifecycle()
        );
        assert!(!services.accepts_children());
        assert!(services.prepare().is_err());
        let _ = block_on(shutdown.wait_services_settled()).unwrap();
        let _ = shutdown.stop_services().unwrap();
        assert_eq!(MimeServiceLifecycle::Stopped, services.lifecycle());
    }

    #[test]
    fn test_shutdown_panic_releases_stop_claim_for_retry() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 1 }).unwrap();
        let shutdown_calls = Arc::new(AtomicUsize::new(0));
        let execution = Arc::new(LateSubmissionExecutionPort {
            worker_port: Arc::new(FixedMimeExecutionPort::new(1, 1)),
            shutdown_calls: Arc::clone(&shutdown_calls),
            before_shutdown: Mutex::new(Some(Box::new(|| panic!("first stop panics")))),
        });
        let services = MimeExecutionServices::new(manager, group, execution, 16).unwrap();
        services.prepare().unwrap();
        let shutdown = services.shutdown();
        let _ = block_on(shutdown.wait_services_settled()).unwrap();

        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _ = shutdown.stop_services();
            }))
            .is_err()
        );
        assert!(shutdown.stop_services().unwrap().services_stopped);
        assert_eq!(1, shutdown_calls.load(Ordering::SeqCst));
    }

    #[test]
    fn test_concurrent_stop_executes_port_shutdown_once() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 1 }).unwrap();
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let shutdown_calls = Arc::new(AtomicUsize::new(0));
        let execution = Arc::new(LateSubmissionExecutionPort {
            worker_port: Arc::new(FixedMimeExecutionPort::new(1, 1)),
            shutdown_calls: Arc::clone(&shutdown_calls),
            before_shutdown: Mutex::new(Some(Box::new(move || {
                started_tx.send(()).unwrap();
                release_rx.recv().unwrap();
            }))),
        });
        let services = MimeExecutionServices::new(manager, group, execution, 16).unwrap();
        services.prepare().unwrap();
        let observer = services.shutdown();
        let _ = block_on(observer.wait_services_settled()).unwrap();
        let first_observer = observer.clone();
        let first = thread::spawn(move || first_observer.stop_services());
        started_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        assert!(matches!(
            observer.stop_services(),
            Err(MimeError::OperationShutdownPending)
        ));
        release_tx.send(()).unwrap();
        assert!(first.join().unwrap().unwrap().services_stopped);
        assert_eq!(1, shutdown_calls.load(Ordering::SeqCst));
    }

    #[test]
    fn test_prepare_panic_releases_prepare_claim_for_retry() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 1 }).unwrap();
        let execution = Arc::new(ControlledPrepareExecutionPort {
            worker_port: Arc::new(FixedMimeExecutionPort::new(1, 1)),
            during_prepare: Mutex::new(Some(Box::new(|| panic!("first prepare panics")))),
            shutdown_calls: Arc::new(AtomicUsize::new(0)),
            prepare_error: AtomicBool::new(false),
        });
        let services = MimeExecutionServices::new(manager, group, execution, 16).unwrap();
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| services.prepare())).is_err()
        );
        assert_eq!(MimeServiceLifecycle::NotStarted, services.lifecycle());
        services.prepare().unwrap();
        assert_eq!(MimeServiceLifecycle::Running, services.lifecycle());
    }

    #[test]
    fn test_prepare_error_after_reentrant_shutdown_restores_closed_without_start() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 1 }).unwrap();
        let (observer_tx, observer_rx) = mpsc::channel();
        let execution = Arc::new(ControlledPrepareExecutionPort {
            worker_port: Arc::new(FixedMimeExecutionPort::new(1, 1)),
            during_prepare: Mutex::new(None),
            shutdown_calls: Arc::new(AtomicUsize::new(0)),
            prepare_error: AtomicBool::new(true),
        });
        let services = MimeExecutionServices::new(
            manager,
            group,
            Arc::clone(&execution) as Arc<dyn MimeExecutionPort>,
            16,
        )
        .unwrap();
        let callback_services = services.clone();
        *execution.during_prepare.lock().unwrap() = Some(Box::new(move || {
            observer_tx.send(callback_services.shutdown()).unwrap();
        }));

        assert!(services.prepare().is_err());
        let observer = observer_rx.recv().unwrap();
        assert_eq!(
            MimeServiceLifecycle::ClosedWithoutStart,
            services.lifecycle()
        );
        assert!(!services.accepts_children());
        let _ = block_on(observer.wait_services_settled()).unwrap();
        assert!(observer.stop_services().unwrap().services_stopped);
    }

    #[test]
    fn test_prepare_unwind_after_reentrant_shutdown_restores_closed_without_start() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 1 }).unwrap();
        let (observer_tx, observer_rx) = mpsc::channel();
        let execution = Arc::new(ControlledPrepareExecutionPort {
            worker_port: Arc::new(FixedMimeExecutionPort::new(1, 1)),
            during_prepare: Mutex::new(None),
            shutdown_calls: Arc::new(AtomicUsize::new(0)),
            prepare_error: AtomicBool::new(false),
        });
        let services = MimeExecutionServices::new(
            manager,
            group,
            Arc::clone(&execution) as Arc<dyn MimeExecutionPort>,
            16,
        )
        .unwrap();
        let callback_services = services.clone();
        *execution.during_prepare.lock().unwrap() = Some(Box::new(move || {
            observer_tx.send(callback_services.shutdown()).unwrap();
            panic!("prepare unwinds after requesting shutdown");
        }));

        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| services.prepare())).is_err()
        );
        let observer = observer_rx.recv().unwrap();
        assert_eq!(
            MimeServiceLifecycle::ClosedWithoutStart,
            services.lifecycle()
        );
        assert!(!services.accepts_children());
        let _ = block_on(observer.wait_services_settled()).unwrap();
        assert!(observer.stop_services().unwrap().services_stopped);
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
        let operation = block_on(
            group.try_submit_async(Default::default(), move |_| async move {
                started_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                Ok::<_, ()>(())
            }),
        )
        .unwrap();
        services.track_observer(operation.observer());
        started_rx
            .recv()
            .expect("operation should enter its blocked body");

        let shutdown = services.shutdown();
        let mut settled_future = Box::pin(shutdown.wait_services_settled());
        let waker = Waker::from(Arc::new(ThreadWaker(thread::current())));
        let mut context = Context::from_waker(&waker);
        assert!(matches!(
            settled_future.as_mut().poll(&mut context),
            Poll::Pending
        ));
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
        assert!(matches!(
            group_shutdown.as_mut().poll(&mut context),
            Poll::Pending
        ));
        drop(operation);
        let group_report = block_on(group_shutdown).unwrap();
        assert_eq!(0, group_report.snapshot.active_operations);
        assert_eq!(0, group_report.snapshot.queued_operations);
        assert_eq!(1, shutdown_calls.load(Ordering::SeqCst));
    }

    #[test]
    fn test_settled_tracked_observer_does_not_mask_active_direct_group_scope() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 2 }).unwrap();
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

        let mut completed = block_on(
            group.try_submit_async(Default::default(), |_| async { Ok::<_, ()>("settled") }),
        )
        .unwrap();
        services.track_observer(completed.observer());
        assert!(matches!(
            block_on(completed.wait_outcome()).unwrap(),
            OperationOutcome::Succeeded("settled")
        ));
        block_on(completed.wait_settlement());
        drop(completed);

        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let mut direct_parent = block_on(group.try_submit_async(
            Default::default(),
            move |_| async move {
                started_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                Ok::<_, ()>(())
            },
        ))
        .unwrap();
        started_rx.recv().unwrap();

        let shutdown = services.shutdown();
        assert!(
            block_on(shutdown.wait_services_settled())
                .unwrap()
                .operations_settled
        );
        let first_stop = shutdown.stop_services();

        release_tx.send(()).unwrap();
        assert!(matches!(
            block_on(direct_parent.wait_outcome()).unwrap(),
            OperationOutcome::Succeeded(())
        ));
        block_on(direct_parent.wait_settlement());
        drop(direct_parent);

        assert!(matches!(
            first_stop,
            Err(MimeError::OperationShutdownPending)
        ));
        assert_eq!(0, shutdown_calls.load(Ordering::SeqCst));
        assert!(shutdown.stop_services().unwrap().services_stopped);
        let report = block_on(shutdown.wait_group_shutdown()).unwrap();
        assert_eq!(0, report.snapshot.active_operations);
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
        let mut operation = block_on(group.try_submit_async(
            Default::default(),
            move |_| async move {
                started_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                Ok::<_, ()>(())
            },
        ))
        .unwrap();
        started_rx.recv().unwrap();

        let shutdown = services.shutdown();
        assert!(
            block_on(shutdown.wait_services_settled())
                .unwrap()
                .operations_settled
        );
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
            block_on(group.try_submit_async(Default::default(), |_| async { Ok::<_, ()>("done") }))
                .unwrap();
        services.track_observer(completed.observer());
        assert!(matches!(
            block_on(completed.wait_outcome()).unwrap(),
            OperationOutcome::Succeeded("done")
        ));
        block_on(completed.wait_settlement());

        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let pending = block_on(
            group.try_submit_async(Default::default(), move |_| async move {
                started_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                Ok::<_, ()>("released")
            }),
        )
        .unwrap();
        services.track_observer(pending.observer());
        started_rx.recv().expect("second operation should start");

        let shutdown = services.shutdown();
        let mut settled_future = Box::pin(shutdown.wait_services_settled());
        let waker = Waker::from(Arc::new(ThreadWaker(thread::current())));
        let mut context = Context::from_waker(&waker);
        assert!(matches!(
            settled_future.as_mut().poll(&mut context),
            Poll::Pending
        ));
        assert!(!shutdown.snapshot().operations_settled);

        release_tx.send(()).unwrap();
        assert!(block_on(settled_future).unwrap().operations_settled);
        let _ = shutdown.stop_services().unwrap();

        let mut group_shutdown = Box::pin(shutdown.wait_group_shutdown());
        let waker = Waker::from(Arc::new(ThreadWaker(thread::current())));
        let mut context = Context::from_waker(&waker);
        assert!(matches!(
            group_shutdown.as_mut().poll(&mut context),
            Poll::Pending
        ));
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
        let mut parent = block_on(group.try_submit_async(
            Default::default(),
            move |scope| async move {
                parent_started_tx.send(()).unwrap();
                release_parent_rx.recv().unwrap();

                let child = scope
                    .manager()
                    .as_async()
                    .try_submit(Default::default(), move |child_scope| async move {
                        stage_owned_in(&child_scope, child_input, child_limits, child_read_tree)
                            .await
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
            },
        ))
        .unwrap();
        parent_started_rx
            .recv()
            .expect("direct parent should enter its barrier");

        let shutdown = services.shutdown();
        assert!(
            block_on(shutdown.wait_services_settled())
                .unwrap()
                .operations_settled
        );
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
        assert_eq!(
            recoveries[0].id(),
            shutdown.recovery_handles_snapshot()[0].id()
        );
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
    fn test_shutdown_rejects_late_external_submission_and_preserves_recovery_and_held_outcome() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 4 }).unwrap();
        let external_parent_group = manager.group(GroupOptions { capacity: 2 }).unwrap();
        let (external_parent_manager_tx, external_parent_manager_rx) = mpsc::channel();
        let (release_external_parent_tx, release_external_parent_rx) = mpsc::channel();
        let mut external_parent = block_on(external_parent_group.try_submit_async(
            Default::default(),
            move |scope| async move {
                external_parent_manager_tx.send(scope.manager()).unwrap();
                release_external_parent_rx.recv().unwrap();
                Ok::<_, ()>(())
            },
        ))
        .unwrap();
        let external_parent_manager = external_parent_manager_rx.recv().unwrap();
        let _release_external_parent = ReleaseOnDrop(release_external_parent_tx.clone());
        let external_group_view = external_parent_manager.with_group(&group).unwrap();

        let worker_port = Arc::new(FixedMimeExecutionPort::new(1, 4));
        let shutdown_calls = Arc::new(AtomicUsize::new(0));
        let late_submission_rejected = Arc::new(AtomicBool::new(false));
        let worker_available_before_shutdown = Arc::new(AtomicBool::new(false));
        let late_group_view = external_group_view.clone();
        let late_worker_port = Arc::clone(&worker_port);
        let late_submission_rejected_in_hook = Arc::clone(&late_submission_rejected);
        let worker_available_in_hook = Arc::clone(&worker_available_before_shutdown);
        let execution = Arc::new(LateSubmissionExecutionPort {
            worker_port: Arc::clone(&worker_port),
            shutdown_calls: Arc::clone(&shutdown_calls),
            before_shutdown: Mutex::new(Some(Box::new(move || {
                let error = match late_group_view
                    .as_async()
                    .try_submit(Default::default(), |_| async { Ok::<_, ()>(()) })
                {
                    Ok(_) => {
                        panic!("late external admission must fail after stop closes the group")
                    }
                    Err(error) => error,
                };
                assert_eq!(AdmissionError::Closed, error.reason());
                late_submission_rejected_in_hook.store(true, Ordering::SeqCst);

                let (job_done_tx, job_done_rx) = mpsc::channel();
                assert!(
                    late_worker_port
                        .try_dispatch(Box::new(move || job_done_tx.send(()).unwrap()))
                        .is_ok()
                );
                job_done_rx
                    .recv_timeout(std::time::Duration::from_secs(2))
                    .expect("worker must still run until after late admission is rejected");
                worker_available_in_hook.store(true, Ordering::SeqCst);
            }))),
        });
        let services = MimeExecutionServices::new(manager, group.clone(), execution, 1024).unwrap();
        services.prepare().unwrap();
        let limits = ManagedStagingLimits {
            max_bytes: 128,
            budget: services.staging_budget(),
            worker: MimeExecutionWorker::new(worker_port),
        };
        let input = OwnedMimeInput::bytes(
            Arc::from(b"retry shutdown cleanup".as_slice()),
            InputExtent::Complete,
        );
        let read_tree = services.new_read_tree(128);
        let body_read_tree = Arc::clone(&read_tree);
        let mut operation = block_on(
            group.try_submit_async(Default::default(), move |scope| async move {
                stage_owned_in(&scope, input, limits, body_read_tree).await
            }),
        )
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
        assert!(super::super::is_unresolved_recovery(
            &settlement.recoveries[0]
        ));

        let mut later_operation =
            block_on(group.try_submit_async(Default::default(), |_| async { Ok::<_, ()>(1) }))
                .unwrap();
        services.track_observer(later_operation.observer());
        block_on(later_operation.wait_settlement());

        let shutdown = services.shutdown();
        let _ = block_on(shutdown.wait_services_settled()).unwrap();
        assert_eq!(shutdown.snapshot().unresolved_recoveries, 1);
        let handles = shutdown.recovery_handles_snapshot();
        assert_eq!(1, handles.len(), "group ledger includes the recovery owner");
        assert_eq!(settlement.recoveries[0].id(), handles[0].id());
        drop(operation);
        assert_eq!(
            handles[0].id(),
            shutdown.recovery_handles_snapshot()[0].id()
        );
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
        assert!(!late_submission_rejected.load(Ordering::SeqCst));

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
        assert!(late_submission_rejected.load(Ordering::SeqCst));
        assert!(worker_available_before_shutdown.load(Ordering::SeqCst));
        assert!(matches!(
            block_on(later_operation.wait_outcome()).unwrap(),
            OperationOutcome::Succeeded(1)
        ));
        release_external_parent_tx.send(()).unwrap();
        assert!(matches!(
            block_on(external_parent.wait_outcome()).unwrap(),
            OperationOutcome::Succeeded(())
        ));
        block_on(external_parent.wait_settlement());
    }

    #[cfg(unix)]
    #[test]
    fn test_creation_recovery_blocks_shutdown_and_survives_typed_handle_drop() {
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
        let limits = ManagedStagingLimits {
            max_bytes: 128,
            budget: services.staging_budget(),
            worker: MimeExecutionWorker::new(worker_port),
        };
        let blocker_path = Arc::new(std::sync::Mutex::new(None));
        let blocker_path_for_create = Arc::clone(&blocker_path);
        let mut operation = block_on(group.try_submit_async(
            Default::default(),
            move |scope| async move {
                let reservation = scope.reserve_resource().unwrap();
                create_empty_staged_with(&scope, reservation, &limits, move || {
                    let fault = qubit_local_files::test_support::install_test_fault(
                        "host-temp-file-recovery-clone-cleanup",
                    )
                    .expect("creation and initial cleanup fault should install");
                    let result = LocalFileSystem::host()?
                        .create_temp_file_with_options(
                            &LocalTempFileOptions::new()
                                .with_privacy(LocalPrivacyRequirement::PlatformDefault),
                        )
                        .map_err(Box::new);
                    if let Err(error) = &result
                        && let Some(recovery) = error.temp_recovery()
                    {
                        let blocker = recovery
                            .path()
                            .parent()
                            .unwrap()
                            .join("shutdown-recovery-blocker");
                        fs::write(&blocker, b"retain failed-creation recovery").unwrap();
                        *blocker_path_for_create.lock().unwrap() = Some(blocker);
                    }
                    drop(fault);
                    result
                })
                .await
            },
        ))
        .unwrap();
        let operation_observer = operation.observer();
        services.track_observer(operation_observer.clone());
        assert!(matches!(
            block_on(operation.wait_outcome()).unwrap(),
            OperationOutcome::Failed(MimeError::Io(_))
        ));

        let shutdown = services.shutdown();
        assert!(matches!(
            shutdown.stop_services(),
            Err(MimeError::OperationShutdownPending)
        ));
        assert_eq!(0, shutdown_calls.load(Ordering::SeqCst));

        drop(operation);
        let settled = block_on(shutdown.wait_services_settled()).unwrap();
        assert!(settled.operations_settled);
        assert_eq!(1, settled.unresolved_recoveries);
        let settlement = block_on(operation_observer.wait_settlement());
        assert_eq!(SettlementStatus::RecoveryRequired, settlement.status);
        assert_eq!(1, settlement.recoveries.len());

        let handles = shutdown.recovery_handles_snapshot();
        assert_eq!(1, handles.len());
        assert_eq!(settlement.recoveries[0].id(), handles[0].id());
        assert_eq!(
            handles[0].id(),
            shutdown.recovery_handles_snapshot()[0].id()
        );
        assert!(matches!(
            shutdown.stop_services(),
            Err(MimeError::OperationShutdownPending)
        ));
        assert_eq!(0, shutdown_calls.load(Ordering::SeqCst));

        let blocker = blocker_path
            .lock()
            .unwrap()
            .take()
            .expect("recovery blocker path");
        fs::remove_file(blocker).unwrap();
        let recovery = handles[0].clone();
        let attempt = recovery.try_retry().unwrap();
        assert!(block_on(attempt.wait_async()).failures.is_empty());
        assert_eq!(0, shutdown.snapshot().unresolved_recoveries);

        assert!(shutdown.stop_services().unwrap().services_stopped);
        assert_eq!(1, shutdown_calls.load(Ordering::SeqCst));
        let report = block_on(shutdown.wait_group_shutdown()).unwrap();
        assert_eq!(0, report.snapshot.active_operations);
        assert_eq!(0, report.snapshot.queued_operations);
    }
}
