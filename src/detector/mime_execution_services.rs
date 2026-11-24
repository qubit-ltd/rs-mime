// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow coverage-cfg
//! MIME-owned ports and shared operation context for managed execution.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU8;
use std::sync::atomic::Ordering;

use qubit_operation::AsyncOperationGroup;
use qubit_operation::AsyncOperationManager;
use qubit_operation::AsyncOperationObserver;
use qubit_operation::AsyncShutdownHandle;
use qubit_operation::OperationGroup;
use qubit_operation::OperationId;
use qubit_operation::OperationManager;
use qubit_operation::OperationScope;
use qubit_operation::RecoveryHandle;
use qubit_operation::RecoveryPhase;
use qubit_operation::ResourceManageErrorKind;

use super::internal::mime_budget::BudgetPermit;
use super::internal::mime_budget::Limits;
use super::internal::mime_budget::MimeBudget;
use super::internal::mime_budget::PendingReadTreeBinding;
use super::internal::mime_budget::ReadTreeContext;
use super::internal::mime_budget::ReadTreeRegistry;
use super::internal::mime_budget::StagingBudget;

mod mime_execution_dispatch_failure;
mod mime_execution_dispatch_failure_kind;
mod mime_execution_port;
mod mime_service_lifecycle;
mod mime_shutdown_observer;
mod mime_shutdown_snapshot;

pub use mime_execution_dispatch_failure::MimeExecutionDispatchFailure;
pub use mime_execution_dispatch_failure_kind::MimeExecutionDispatchFailureKind;
pub use mime_execution_port::MimeExecutionPort;
pub use mime_service_lifecycle::MimeServiceLifecycle;
pub use mime_shutdown_observer::MimeShutdownObserver;
pub use mime_shutdown_snapshot::MimeShutdownSnapshot;

/// Services shared by MIME requests. Runtime construction is explicit so a
/// closed service cannot silently create replacement workers.
///
/// # Examples
///
/// Create the operation group and execution adapter before constructing the
/// shared services. Construction leaves workers stopped until `prepare` is
/// called.
///
/// ```
/// use std::sync::Arc;
/// use qubit_mime::{MimeError, MimeExecutionPort, MimeExecutionServices};
/// use qubit_operation::{GroupOptions, OperationManager};
///
/// #[derive(Debug)]
/// struct NoopPort;
///
/// impl MimeExecutionPort for NoopPort {
///     fn prepare(&self) -> Result<(), MimeError> { Ok(()) }
///     fn shutdown(&self) -> Result<(), MimeError> { Ok(()) }
/// }
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let manager = OperationManager::new(Default::default())?;
/// let group = manager.group(GroupOptions { capacity: 1 })?;
/// let services = MimeExecutionServices::new(manager, group, Arc::new(NoopPort), 1024)?;
/// assert_eq!(services.max_read_bytes(), 1024);
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct MimeExecutionServices {
    /// Application-shared asynchronous manager.
    #[allow(dead_code)]
    pub(crate) manager: AsyncOperationManager,
    /// Synchronous view of the shared manager, used by the sync facade.
    #[allow(dead_code)]
    pub(crate) sync_manager: OperationManager,
    /// MIME's unparented root group.
    pub(crate) root_group: OperationGroup,
    /// Async root group used by MIME async requests.
    pub(crate) async_root_group: AsyncOperationGroup,
    /// MIME-specific execution adapters.
    pub(crate) execution: Arc<dyn MimeExecutionPort>,
    /// Finite cumulative bytes-read limit required by construction.
    pub(crate) max_read_bytes: u64,
    /// Shared limits for owned memory and staging allocations.
    budget: Arc<MimeBudget>,
    /// Read-budget contexts indexed by operation ID.
    read_trees: Arc<ReadTreeRegistry>,
    /// Lifecycle state: new, prepared, shutting down, or stopped.
    lifecycle: Arc<AtomicU8>,
    /// Whether new root-level submissions are closed.
    top_closed: Arc<AtomicBool>,
    /// Whether stop has frozen all MIME-mediated child submissions.
    admission_frozen: Arc<AtomicBool>,
    /// Serializes lifecycle transitions.
    lifecycle_lock: Arc<Mutex<()>>,
    /// Claims the one blocking execution shutdown call without holding locks
    /// that a shutdown callback can re-enter.
    stopping: Arc<AtomicBool>,
    /// Prevents submissions racing with settlement and shutdown.
    submission_lock: Arc<Mutex<()>>,
    /// Observers for operations that have not yet been harvested.
    observers: Arc<Mutex<Vec<AsyncOperationObserver>>>,
    /// Recovery handles retained until explicitly resolved or transferred.
    recoveries: Arc<Mutex<Vec<RecoveryHandle>>>,
    /// Handle for draining the MIME operation group after workers stop.
    group_shutdown: Arc<Mutex<Option<AsyncShutdownHandle>>>,
    /// Whether all tracked operations have published settlement.
    operations_settled: Arc<AtomicBool>,
}

impl MimeExecutionServices {
    /// Returns the immutable execution adapter shared by this service.
    #[inline]
    #[must_use]
    pub fn execution(&self) -> &Arc<dyn MimeExecutionPort> {
        &self.execution
    }

    /// Returns the finite cumulative byte-read limit shared by this service.
    #[inline]
    #[must_use]
    pub const fn max_read_bytes(&self) -> u64 {
        self.max_read_bytes
    }

    /// Returns the shared service lifecycle phase.
    #[must_use]
    pub fn lifecycle(&self) -> MimeServiceLifecycle {
        MimeServiceLifecycle::from_raw(self.lifecycle.load(Ordering::Acquire))
    }

    /// Poisons read-tree registration so integration coverage can verify lock
    /// recovery.
    ///
    /// This method exists only in coverage builds and is not part of the
    /// published API.
    #[cfg(coverage)]
    #[doc(hidden)]
    pub fn poison_read_tree_registration_lock_for_coverage(&self) {
        self.read_trees.poison_registrations_for_coverage();
    }

    /// Creates services with a finite nonzero cumulative read limit.
    ///
    /// # Parameters
    /// - `sync_manager`: Shared operation manager used by MIME operations.
    /// - `root_group`: MIME-owned operation group.
    /// - `execution`: MIME-specific worker adapter.
    /// - `max_read_bytes`: Maximum cumulative provider bytes read.
    ///
    /// # Returns
    /// Prepared service state, with workers still stopped.
    ///
    /// # Errors
    /// Returns [`crate::MimeError::InvalidOperationBudget`] when the read
    /// limit is zero.
    pub fn new(
        sync_manager: OperationManager,
        root_group: OperationGroup,
        execution: Arc<dyn MimeExecutionPort>,
        max_read_bytes: u64,
    ) -> Result<Self, crate::MimeError> {
        if max_read_bytes == 0 {
            return Err(crate::MimeError::InvalidOperationBudget);
        }
        let manager = sync_manager.as_async();
        let async_root_group = root_group.as_async();
        Ok(Self {
            manager,
            sync_manager,
            root_group,
            async_root_group,
            execution,
            max_read_bytes,
            budget: MimeBudget::new(Limits {
                owned: 16 * 1024 * 1024,
                staging: 256 * 1024 * 1024,
            }),
            read_trees: Arc::new(ReadTreeRegistry::default()),
            lifecycle: Arc::new(AtomicU8::new(0)),
            top_closed: Arc::new(AtomicBool::new(false)),
            admission_frozen: Arc::new(AtomicBool::new(false)),
            lifecycle_lock: Arc::new(Mutex::new(())),
            stopping: Arc::new(AtomicBool::new(false)),
            submission_lock: Arc::new(Mutex::new(())),
            observers: Arc::new(Mutex::new(Vec::new())),
            recoveries: Arc::new(Mutex::new(Vec::new())),
            group_shutdown: Arc::new(Mutex::new(None)),
            operations_settled: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Starts MIME-owned fixed-capacity workers explicitly.
    pub fn prepare(&self) -> Result<(), crate::MimeError> {
        let _guard = self.lifecycle_lock.lock().unwrap_or_else(|e| e.into_inner());
        match self.lifecycle() {
            MimeServiceLifecycle::NotStarted => {
                self.execution.prepare()?;
                self.lifecycle
                    .store(MimeServiceLifecycle::Running as u8, Ordering::Release);
                Ok(())
            }
            MimeServiceLifecycle::Running => Ok(()),
            _ => Err(crate::MimeError::ExecutionServicesRequired),
        }
    }

    /// Closes new top-level MIME submissions and returns a repeatable observer.
    /// Await operation settlements, call `stop_services` from an external
    /// synchronous context, then observe group Drain separately.
    pub fn shutdown(&self) -> MimeShutdownObserver {
        let _guard = self.lifecycle_lock.lock().unwrap_or_else(|e| e.into_inner());
        if !matches!(
            self.lifecycle(),
            MimeServiceLifecycle::Stopped | MimeServiceLifecycle::ClosedWithoutStart
        ) {
            self.top_closed.store(true, Ordering::Release);
            let next = if self.lifecycle() == MimeServiceLifecycle::NotStarted {
                MimeServiceLifecycle::ClosedWithoutStart
            } else {
                MimeServiceLifecycle::Draining
            };
            self.lifecycle.store(next as u8, Ordering::Release);
        }
        MimeShutdownObserver { services: self.clone() }
    }

    /// Locks submissions while a caller performs a coordinated transition.
    ///
    /// # Returns
    /// A guard that unlocks submissions when dropped.
    pub(in crate::detector) fn submission_guard(&self) -> std::sync::MutexGuard<'_, ()> {
        self.submission_lock.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Retains an observer and harvests recovery handles from older settled
    /// observers.
    ///
    /// # Parameters
    /// - `observer`: Observer for a newly submitted operation.
    pub(in crate::detector) fn track_observer(&self, observer: AsyncOperationObserver) {
        self.operations_settled.store(false, Ordering::Release);
        let mut observers = self.observers.lock().unwrap_or_else(|e| e.into_inner());
        observers.retain(|item| {
            let snapshot = item.snapshot();
            if let Some(report) = snapshot.settlement {
                self.track_recoveries(&report.recoveries);
                false
            } else {
                true
            }
        });
        observers.push(observer);
    }

    /// Adds unique recovery handles that still require caller action.
    ///
    /// # Parameters
    /// - `recoveries`: Recovery handles published by an operation settlement.
    pub(crate) fn track_recoveries(&self, recoveries: &[RecoveryHandle]) {
        let mut tracked = self.recoveries.lock().unwrap_or_else(|e| e.into_inner());
        tracked.retain(is_unresolved_recovery);
        for recovery in recoveries {
            if !tracked.iter().any(|item| item.id() == recovery.id()) {
                tracked.push(recovery.clone());
            }
        }
    }

    /// Counts recoveries that are neither resolved nor transferred.
    ///
    /// # Returns
    /// The count of outstanding handles requiring explicit action.
    pub(crate) fn unresolved_recovery_count(&self) -> usize {
        self.unresolved_recovery_handles().len()
    }

    /// Returns unresolved recovery ownership from the operation group's
    /// authoritative ledger, including child scopes not submitted by MIME.
    pub(crate) fn unresolved_recovery_handles(&self) -> Vec<RecoveryHandle> {
        let mut handles = self.root_group.recovery_handles_snapshot();
        handles.retain(is_unresolved_recovery);
        handles
    }

    /// Returns whether the group still has unsettled operation scopes.
    ///
    /// The group ledger excludes operations whose bodies and settlement have
    /// completed, even when callers retain their typed outcomes.
    pub(crate) fn has_unsettled_operations(&self) -> bool {
        self.root_group.unsettled_operation_count() != 0
    }

    /// Returns whether workers are running and root submissions remain open.
    #[inline]
    #[must_use]
    pub(in crate::detector) fn is_prepared(&self) -> bool {
        self.lifecycle() == MimeServiceLifecycle::Running && !self.top_closed.load(Ordering::Acquire)
    }
    /// Returns whether child submissions can still be accepted.
    #[inline]
    #[must_use]
    pub(in crate::detector) fn accepts_children(&self) -> bool {
        match self.lifecycle() {
            MimeServiceLifecycle::Running => true,
            MimeServiceLifecycle::Draining => !self.admission_frozen.load(Ordering::Acquire),
            MimeServiceLifecycle::NotStarted
            | MimeServiceLifecycle::ClosedWithoutStart
            | MimeServiceLifecycle::Stopped => false,
        }
    }
    /// Reserves owned-memory budget for an input allocation.
    ///
    /// # Parameters
    /// - `bytes`: Number of input bytes to reserve.
    ///
    /// # Returns
    /// A permit when the allocation fits the owned-memory budget.
    #[inline]
    pub(in crate::detector) fn reserve_owned(&self, bytes: usize) -> Option<BudgetPermit> {
        self.budget.reserve_owned(bytes)
    }

    /// Creates a child read-budget context for a parent operation after
    /// admission has already succeeded.
    ///
    /// # Parameters
    /// - `parent`: Parent operation whose resource scope will retain the tree.
    /// - `tree_limit`: Cumulative read allowance for the operation tree.
    /// - `scope_limit`: Read allowance requested for this child.
    ///
    /// # Returns
    /// An independent child context charged to the parent resource scope.
    #[allow(dead_code)]
    #[inline]
    pub(crate) fn read_tree_for_parent(
        &self,
        parent: &OperationScope,
        tree_limit: u64,
        scope_limit: u64,
    ) -> Result<Arc<ReadTreeContext>, ResourceManageErrorKind> {
        self.read_trees.child_for_parent(parent, tree_limit, scope_limit)
    }

    /// Borrows an existing parent tree without reserving another resource.
    ///
    /// # Parameters
    /// - `parent`: Parent operation whose tree should be reused.
    /// - `scope_limit`: Read allowance requested for this child.
    ///
    /// # Returns
    /// The existing child context when the parent tree can cover the request.
    #[inline]
    pub(crate) fn existing_read_tree_for_parent(
        &self,
        parent: &OperationScope,
        scope_limit: u64,
    ) -> Option<Arc<ReadTreeContext>> {
        self.read_trees.existing_child_for_parent(parent, scope_limit)
    }

    /// Associates a child with its parent's tree without reserving a parent
    /// resource slot. A later provider read promotes the association.
    ///
    /// # Parameters
    /// - `parent`: Parent operation whose tree should own the child.
    /// - `tree_limit`: Maximum resources allowed in the associated tree.
    /// - `scope_limit`: Maximum resources allowed for this child scope.
    ///
    /// # Returns
    /// The child context associated with the parent's read tree.
    #[inline]
    pub(crate) fn associate_read_tree_for_parent(
        &self,
        parent: &OperationScope,
        tree_limit: u64,
        scope_limit: u64,
    ) -> Arc<ReadTreeContext> {
        self.read_trees
            .associate_child_for_parent(parent, tree_limit, scope_limit)
    }

    /// Prepares a parent read-tree binding that an admitted body commits.
    ///
    /// # Parameters
    /// - `parent`: Parent operation whose resource scope will retain the tree.
    /// - `tree_limit`: Cumulative read allowance for the operation tree.
    /// - `scope_limit`: Read allowance requested for this child.
    ///
    /// # Returns
    /// A pending binding that releases reserved capacity if admission fails
    /// and commits the parent anchor when its body starts.
    ///
    /// # Errors
    /// Returns the parent resource-scope failure when capacity cannot be
    /// reserved.
    #[inline]
    pub(crate) fn prepare_read_tree_for_parent(
        &self,
        parent: &OperationScope,
        tree_limit: u64,
        scope_limit: u64,
    ) -> Result<PendingReadTreeBinding, ResourceManageErrorKind> {
        self.read_trees
            .reserve_child_for_parent(parent, tree_limit, scope_limit)
    }

    /// Creates a standalone read-budget context with the given limit.
    ///
    /// # Parameters
    /// - `limit`: Maximum provider bytes the context may charge.
    ///
    /// # Returns
    /// An independent read-budget context.
    #[inline]
    pub(crate) fn new_read_tree(&self, limit: u64) -> Arc<ReadTreeContext> {
        ReadTreeContext::new(limit)
    }

    /// Retrieves the read-budget context already bound to an operation.
    ///
    /// Staging and refinement must use this context so their provider reads
    /// share the request's cumulative allowance.
    ///
    /// # Parameters
    /// - `operation`: Operation whose read-budget context is requested.
    ///
    /// # Returns
    /// The context already bound to the operation.
    ///
    /// # Errors
    /// Returns [`crate::MimeError::ExecutionServicesRequired`] if no context is
    /// bound.
    #[inline]
    pub(crate) fn read_tree_for_operation(
        &self,
        operation: OperationId,
    ) -> Result<Arc<ReadTreeContext>, crate::MimeError> {
        self.read_trees
            .for_operation(operation)
            .ok_or(crate::MimeError::ExecutionServicesRequired)
    }

    /// Binds a read-budget context to an operation identity.
    ///
    /// # Parameters
    /// - `operation`: Operation receiving the context.
    /// - `tree`: Shared read-budget context to bind.
    #[inline]
    pub(crate) fn bind_read_tree(&self, operation: OperationId, tree: &Arc<ReadTreeContext>) {
        self.read_trees.bind(operation, tree);
    }
    /// Reserves owned-memory budget for a shared byte allocation.
    ///
    /// # Parameters
    /// - `bytes`: Shared allocation to account for.
    ///
    /// # Returns
    /// A permit when the allocation fits the owned-memory budget.
    #[inline]
    pub(in crate::detector) fn reserve_allocation(&self, bytes: &Arc<[u8]>) -> Option<BudgetPermit> {
        self.budget.reserve_allocation(bytes)
    }
    /// Reserves staging budget for a temporary-file write.
    ///
    /// # Parameters
    /// - `bytes`: Number of staged bytes to reserve.
    ///
    /// # Returns
    /// A permit when the staged bytes fit the budget.
    #[allow(dead_code)]
    #[inline]
    pub(in crate::detector) fn reserve_staging(&self, bytes: u64) -> Option<BudgetPermit> {
        self.budget.reserve_staging(bytes)
    }

    /// Creates the staging-budget adapter used by owned staging requests.
    #[inline]
    pub(crate) fn staging_budget(&self) -> StagingBudget {
        StagingBudget::from_budget(Arc::clone(&self.budget))
    }

    /// Creates a staging-budget adapter associated with a bound request when
    /// one exists. Direct classifier calls retain the service-wide budget.
    ///
    /// # Parameters
    /// - `operation`: Operation whose read tree supplies request-scoped staging
    ///   usage.
    pub(crate) fn staging_budget_for_operation(&self, operation: OperationId) -> StagingBudget {
        let budget = self.staging_budget();
        match self.read_trees.for_operation(operation) {
            Some(tree) => budget.with_usage(tree.staging_usage()),
            None => budget,
        }
    }
}

/// Reports whether a recovery handle still requires resolution or transfer.
///
/// # Parameters
/// - `recovery`: Handle whose current phase is inspected.
///
/// # Returns
/// `true` unless the handle is resolved or transferred.
fn is_unresolved_recovery(recovery: &RecoveryHandle) -> bool {
    !matches!(
        recovery.snapshot().phase,
        RecoveryPhase::Resolved | RecoveryPhase::Transferred
    )
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::sync::Arc;
    use std::sync::Barrier;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;
    use std::sync::mpsc;
    use std::task::Context;
    use std::task::Poll;
    use std::task::Wake;
    use std::task::Waker;

    use qubit_operation::GroupOptions;
    use qubit_operation::OperationManager;

    use super::MimeExecutionServices;
    use crate::MimeError;
    use crate::detector::InputExtent;
    use crate::detector::MimeExecutionPort;
    use crate::detector::MimeRuntime;
    use crate::detector::OwnedMimeInput;

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

    #[derive(Debug)]
    struct NoopPort;

    impl MimeExecutionPort for NoopPort {
        fn prepare(&self) -> Result<(), MimeError> {
            Ok(())
        }

        fn shutdown(&self) -> Result<(), MimeError> {
            Ok(())
        }
    }

    #[derive(Debug)]
    struct FailOncePort(AtomicUsize);

    impl MimeExecutionPort for FailOncePort {
        fn prepare(&self) -> Result<(), MimeError> {
            if self.0.fetch_add(1, Ordering::SeqCst) == 0 {
                Err(MimeError::ExecutionServicesRequired)
            } else {
                Ok(())
            }
        }

        fn shutdown(&self) -> Result<(), MimeError> {
            Ok(())
        }
    }

    #[derive(Debug)]
    struct RetryShutdownPort(AtomicUsize);

    impl MimeExecutionPort for RetryShutdownPort {
        fn prepare(&self) -> Result<(), MimeError> {
            Ok(())
        }

        fn shutdown(&self) -> Result<(), MimeError> {
            if self.0.fetch_add(1, Ordering::SeqCst) == 0 {
                Err(MimeError::ExecutionServicesRequired)
            } else {
                Ok(())
            }
        }
    }

    #[derive(Debug)]
    struct GatedPreparePort {
        entered: Arc<Barrier>,
        release: Arc<Barrier>,
        fail_first: bool,
        calls: AtomicUsize,
    }

    impl MimeExecutionPort for GatedPreparePort {
        fn prepare(&self) -> Result<(), MimeError> {
            let call = self.calls.fetch_add(1, Ordering::SeqCst);
            self.entered.wait();
            self.release.wait();
            if self.fail_first && call == 0 {
                Err(MimeError::ExecutionServicesRequired)
            } else {
                Ok(())
            }
        }

        fn shutdown(&self) -> Result<(), MimeError> {
            Ok(())
        }
    }

    #[test]
    fn test_prepare_does_not_publish_ready_before_execution_port_returns() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 2 }).unwrap();
        let entered = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let port = Arc::new(GatedPreparePort {
            entered: Arc::clone(&entered),
            release: Arc::clone(&release),
            fail_first: false,
            calls: AtomicUsize::new(0),
        });
        let services = MimeExecutionServices::new(manager, group.clone(), port.clone(), 32).unwrap();
        let preparing = services.clone();
        let prepare_thread = std::thread::spawn(move || preparing.prepare());

        entered.wait();
        assert!(
            !services.is_prepared(),
            "workers are not ready until port preparation completes"
        );
        assert!(
            !services.accepts_children(),
            "children are not admissible during preparation"
        );

        release.wait();
        prepare_thread.join().unwrap().unwrap();
        assert!(services.is_prepared());
        assert!(services.accepts_children());
        assert_eq!(1, port.calls.load(Ordering::SeqCst));
    }

    #[test]
    fn test_failed_prepare_keeps_services_unready_and_can_be_retried() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 1 }).unwrap();
        let entered = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let port = Arc::new(GatedPreparePort {
            entered: Arc::clone(&entered),
            release: Arc::clone(&release),
            fail_first: true,
            calls: AtomicUsize::new(0),
        });
        let services = MimeExecutionServices::new(manager, group, port.clone(), 32).unwrap();

        let preparing = services.clone();
        let prepare_thread = std::thread::spawn(move || preparing.prepare());
        entered.wait();
        assert!(!services.is_prepared());
        assert!(!services.accepts_children());
        release.wait();
        assert!(matches!(
            prepare_thread.join().unwrap(),
            Err(MimeError::ExecutionServicesRequired)
        ));
        assert!(!services.is_prepared());
        assert!(!services.accepts_children());

        let retrying = services.clone();
        let retry_thread = std::thread::spawn(move || retrying.prepare());
        entered.wait();
        assert!(!services.is_prepared());
        release.wait();
        retry_thread.join().unwrap().unwrap();
        assert_eq!(2, port.calls.load(Ordering::SeqCst));
        assert!(services.is_prepared());
        assert!(services.accepts_children());
    }

    #[test]
    fn test_root_and_child_submissions_are_rejected_during_prepare() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 4 }).unwrap();
        let entered = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let port = Arc::new(GatedPreparePort {
            entered: Arc::clone(&entered),
            release: Arc::clone(&release),
            fail_first: false,
            calls: AtomicUsize::new(0),
        });
        let services = MimeExecutionServices::new(manager, group.clone(), port, 32).unwrap();
        let runtime = MimeRuntime::builtin()
            .unwrap()
            .with_execution_services(services.clone());

        let (scope_tx, scope_rx) = mpsc::sync_channel(0);
        let (parent_release_tx, parent_release_rx) = mpsc::channel();
        let mut parent = block_on(group.try_submit_async(Default::default(), move |scope| async move {
            scope_tx.send(scope).unwrap();
            parent_release_rx.recv().unwrap();
            Ok::<_, MimeError>(())
        }))
        .unwrap();
        let parent_scope = scope_rx.recv().unwrap();

        let preparing = services.clone();
        let prepare_thread = std::thread::spawn(move || preparing.prepare());
        entered.wait();

        let root_bytes: Arc<[u8]> = Arc::from(b"root input".as_slice());
        let root_result = runtime.try_submit_detection(
            OwnedMimeInput::bytes(Arc::clone(&root_bytes), InputExtent::Complete),
            Default::default(),
        );
        let child_bytes: Arc<[u8]> = Arc::from(b"child input".as_slice());
        let child_result = runtime.try_submit_detection_in(
            &parent_scope,
            OwnedMimeInput::bytes(Arc::clone(&child_bytes), InputExtent::Complete),
            Default::default(),
        );
        for (submission, expected) in [(root_result, root_bytes), (child_result, child_bytes)] {
            let failure = match submission {
                Ok(_) => panic!("preparation in progress must reject submission"),
                Err(failure) => failure,
            };
            assert!(matches!(
                failure.reason,
                crate::detector::MimeSubmitRejection::ExecutionServicesRequired
            ));
            match failure.input {
                OwnedMimeInput::Bytes { content, .. } => {
                    assert!(Arc::ptr_eq(&content, &expected));
                }
                OwnedMimeInput::Provider { .. } => {
                    panic!("returned input should remain byte input")
                }
            }
        }

        release.wait();
        prepare_thread.join().unwrap().unwrap();
        parent_release_tx.send(()).unwrap();
        assert!(matches!(
            block_on(parent.wait_outcome()).unwrap(),
            qubit_operation::OperationOutcome::Succeeded(())
        ));
        assert!(block_on(parent.wait_settlement()).all_released());
        let shutdown = services.shutdown();
        let _ = block_on(shutdown.wait_services_settled()).unwrap();
        let _ = shutdown.stop_services().unwrap();
    }

    #[test]
    fn test_child_admission_rejection_releases_uncommitted_read_tree_binding() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 1 }).unwrap();
        let services = MimeExecutionServices::new(manager, group, Arc::new(NoopPort), 16).unwrap();
        services.prepare().unwrap();
        let runtime = MimeRuntime::builtin()
            .unwrap()
            .with_execution_services(services.clone());
        let body_services = services.clone();
        let body_runtime = runtime.clone();
        let options = qubit_operation::OperationOptions {
            limits: qubit_operation::ScopeLimits {
                resources: 1,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut parent = services
            .async_root_group
            .try_submit(options, move |scope| async move {
                let original: Arc<[u8]> = Arc::from(b"child bytes".as_slice());
                let failure = match body_runtime.try_submit_detection_in(
                    &scope,
                    OwnedMimeInput::bytes(Arc::clone(&original), InputExtent::Complete),
                    Default::default(),
                ) {
                    Err(failure) => failure,
                    Ok(_) => panic!("the occupied capacity-one group must reject its child"),
                };
                let admission_rejected = matches!(failure.reason, crate::detector::MimeSubmitRejection::Admission(_));
                let input_returned = match failure.input {
                    OwnedMimeInput::Bytes { content, .. } => Arc::ptr_eq(&content, &original),
                    OwnedMimeInput::Provider { .. } => false,
                };
                let no_tree_registered = body_services.read_tree_for_operation(scope.id()).is_err();
                let capacity_released = match scope.reserve_resource() {
                    Ok(reservation) => {
                        drop(reservation);
                        true
                    }
                    Err(_) => false,
                };
                Ok::<_, MimeError>((
                    admission_rejected,
                    input_returned,
                    no_tree_registered,
                    capacity_released,
                ))
            })
            .unwrap();
        let result = match block_on(parent.wait_outcome()).unwrap() {
            qubit_operation::OperationOutcome::Succeeded(result) => result,
            other => panic!("parent operation should complete: {other:?}"),
        };
        block_on(parent.wait_settlement());
        assert_eq!((true, true, true, true), result);

        let body_services = services.clone();
        let body_runtime = runtime.clone();
        let sync_options = qubit_operation::OperationOptions {
            limits: qubit_operation::ScopeLimits {
                resources: 1,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut parent = services
            .async_root_group
            .try_submit(sync_options, move |scope| async move {
                let original: Arc<[u8]> = Arc::from(b"sync child bytes".as_slice());
                let failure = match body_runtime.try_submit_detection_sync_in(
                    &scope,
                    OwnedMimeInput::bytes(Arc::clone(&original), InputExtent::Complete),
                    Default::default(),
                ) {
                    Err(failure) => failure,
                    Ok(_) => panic!("the occupied capacity-one group must reject its sync child"),
                };
                let admission_rejected = matches!(failure.reason, crate::detector::MimeSubmitRejection::Admission(_));
                let input_returned = match failure.input {
                    OwnedMimeInput::Bytes { content, .. } => Arc::ptr_eq(&content, &original),
                    OwnedMimeInput::Provider { .. } => false,
                };
                let no_tree_registered = body_services.read_tree_for_operation(scope.id()).is_err();
                let capacity_released = match scope.reserve_resource() {
                    Ok(reservation) => {
                        drop(reservation);
                        true
                    }
                    Err(_) => false,
                };
                Ok::<_, MimeError>((
                    admission_rejected,
                    input_returned,
                    no_tree_registered,
                    capacity_released,
                ))
            })
            .unwrap();
        let result = match block_on(parent.wait_outcome()).unwrap() {
            qubit_operation::OperationOutcome::Succeeded(result) => result,
            other => panic!("sync parent operation should complete: {other:?}"),
        };
        block_on(parent.wait_settlement());
        assert_eq!((true, true, true, true), result);

        let shutdown = services.shutdown();
        let _ = block_on(shutdown.wait_services_settled()).unwrap();
        let _ = shutdown.stop_services().unwrap();
        let _report = block_on(shutdown.wait_group_shutdown()).unwrap();
    }

    #[test]
    fn test_shutdown_waits_for_prepare_and_prevents_late_ready_state() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 1 }).unwrap();
        let entered = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let services = MimeExecutionServices::new(
            manager,
            group,
            Arc::new(GatedPreparePort {
                entered: Arc::clone(&entered),
                release: Arc::clone(&release),
                fail_first: false,
                calls: AtomicUsize::new(0),
            }),
            32,
        )
        .unwrap();

        let preparing = services.clone();
        let prepare_thread = std::thread::spawn(move || preparing.prepare());
        entered.wait();
        let shutdown_services = services.clone();
        let shutdown_contended = Arc::new(Barrier::new(2));
        let shutdown_contended_thread = Arc::clone(&shutdown_contended);
        let shutdown_thread = std::thread::spawn(move || {
            match shutdown_services.lifecycle_lock.try_lock() {
                Err(std::sync::TryLockError::WouldBlock) => {}
                Err(std::sync::TryLockError::Poisoned(error)) => {
                    drop(error.into_inner());
                    panic!("lifecycle lock should not be poisoned");
                }
                Ok(_guard) => panic!("prepare should still hold the lifecycle lock"),
            }
            shutdown_contended_thread.wait();
            shutdown_services.shutdown()
        });
        shutdown_contended.wait();
        release.wait();

        prepare_thread.join().unwrap().unwrap();
        let shutdown = shutdown_thread.join().unwrap();
        assert!(
            !services.is_prepared(),
            "shutdown must close services after preparation finishes"
        );
        assert!(matches!(services.prepare(), Err(MimeError::ExecutionServicesRequired)));
        let _ = block_on(shutdown.wait_services_settled()).unwrap();
        let _ = shutdown.stop_services().unwrap();
    }

    #[test]
    fn test_service_budget_reservations_track_owned_allocations_and_staging() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 1 }).unwrap();
        let services = MimeExecutionServices::new(manager, group, Arc::new(NoopPort), 32).unwrap();
        let allocation: Arc<[u8]> = Arc::from(b"shared allocation".as_slice());

        let first = services.reserve_allocation(&allocation).expect("allocation should fit");
        let alias = services
            .reserve_allocation(&allocation)
            .expect("same allocation should be shared");
        assert_eq!(allocation.len(), services.budget.owned_bytes());
        drop(first);
        assert_eq!(allocation.len(), services.budget.owned_bytes());
        drop(alias);
        assert_eq!(0, services.budget.owned_bytes());

        let staging_limit = 256 * 1024 * 1024;
        let staging = services
            .reserve_staging(staging_limit)
            .expect("exact default staging budget should fit");
        assert_eq!(staging_limit, services.budget.staging_bytes());
        assert!(services.reserve_staging(1).is_none());
        drop(staging);
        assert_eq!(0, services.budget.staging_bytes());

        let clone = services.clone();
        let owned = services
            .reserve_owned(16 * 1024 * 1024)
            .expect("the full owned-memory budget should be reservable");
        assert_eq!(16 * 1024 * 1024, clone.budget.owned_bytes());
        assert!(clone.reserve_owned(1).is_none(), "clones share the same hard limit");
        drop(owned);
        assert_eq!(0, clone.budget.owned_bytes());
        assert!(
            clone.reserve_owned(1).is_some(),
            "dropping the permit releases capacity for every clone"
        );
    }

    #[test]
    fn test_service_prepare_failure_rolls_back_and_retry_is_idempotent() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 1 }).unwrap();
        let port = Arc::new(FailOncePort(AtomicUsize::new(0)));
        let services = MimeExecutionServices::new(manager, group, port.clone(), 32).unwrap();

        assert!(matches!(services.prepare(), Err(MimeError::ExecutionServicesRequired)));
        assert!(!services.is_prepared());
        assert!(!services.accepts_children());
        services.prepare().unwrap();
        assert!(services.is_prepared());
        services.prepare().unwrap();
        assert_eq!(
            2,
            port.0.load(Ordering::SeqCst),
            "prepared state does not restart the port"
        );

        let shutdown = services.shutdown();
        let _ = block_on(shutdown.wait_services_settled()).unwrap();
        let _ = shutdown.stop_services().unwrap();
        assert!(matches!(services.prepare(), Err(MimeError::ExecutionServicesRequired)));
        assert_eq!(
            2,
            port.0.load(Ordering::SeqCst),
            "stopped services cannot prepare again"
        );
    }

    #[test]
    fn test_operation_read_tree_must_be_bound_and_keeps_parent_accounting() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 1 }).unwrap();
        let services = Arc::new(MimeExecutionServices::new(manager, group.clone(), Arc::new(NoopPort), 32).unwrap());
        let body_services = Arc::clone(&services);
        let mut operation = block_on(group.try_submit_async(Default::default(), move |scope| async move {
            assert!(matches!(
                body_services.read_tree_for_operation(scope.id()),
                Err(MimeError::ExecutionServicesRequired)
            ));
            let tree = body_services.read_tree_for_parent(&scope, 16, 16).unwrap();
            body_services.bind_read_tree(scope.id(), &tree);
            let bound = body_services.read_tree_for_operation(scope.id()).unwrap();
            assert!(Arc::ptr_eq(&tree, &bound));
            assert_eq!(16, bound.remaining());
            assert_eq!(4, bound.reserve(4).unwrap().commit(4));
            assert_eq!(12, tree.remaining(), "bound reads consume the shared parent allowance");
            Ok::<_, MimeError>(())
        }))
        .unwrap();
        assert!(matches!(
            block_on(operation.wait_outcome()).unwrap(),
            qubit_operation::OperationOutcome::Succeeded(())
        ));
        assert!(block_on(operation.wait_settlement()).all_released());
        let shutdown = services.shutdown();
        let _ = block_on(shutdown.wait_services_settled()).unwrap();
        let _ = shutdown.stop_services().unwrap();
    }

    #[test]
    fn test_service_recovers_poisoned_lifecycle_and_submission_locks() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 1 }).unwrap();
        let services = MimeExecutionServices::new(manager, group, Arc::new(NoopPort), 32).unwrap();

        let lifecycle = Arc::clone(&services.lifecycle_lock);
        assert!(
            std::thread::spawn(move || {
                let _guard = lifecycle.lock().unwrap();
                panic!("poison lifecycle mutex for recovery test");
            })
            .join()
            .is_err()
        );
        services.prepare().expect("prepare should recover the poisoned lock");
        assert!(services.is_prepared());

        let submission = Arc::clone(&services.submission_lock);
        assert!(
            std::thread::spawn(move || {
                let _guard = submission.lock().unwrap();
                panic!("poison submission mutex for recovery test");
            })
            .join()
            .is_err()
        );
        drop(services.submission_guard());

        let shutdown = services.shutdown();
        assert!(block_on(shutdown.wait_services_settled()).unwrap().operations_settled);
        assert!(shutdown.stop_services().unwrap().services_stopped);
    }

    #[test]
    fn test_service_tracking_and_shutdown_recover_poisoned_tracking_locks() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 1 }).unwrap();
        let services = MimeExecutionServices::new(manager, group.clone(), Arc::new(NoopPort), 32).unwrap();

        for lock in [
            Arc::clone(&services.lifecycle_lock),
            Arc::clone(&services.submission_lock),
        ] {
            assert!(
                std::thread::spawn(move || {
                    let _guard = lock.lock().unwrap();
                    panic!("poison MIME execution service lock");
                })
                .join()
                .is_err()
            );
        }
        let observers = Arc::clone(&services.observers);
        assert!(
            std::thread::spawn(move || {
                let _guard = observers.lock().unwrap();
                panic!("poison MIME execution observer lock");
            })
            .join()
            .is_err()
        );
        let recoveries = Arc::clone(&services.recoveries);
        assert!(
            std::thread::spawn(move || {
                let _guard = recoveries.lock().unwrap();
                panic!("poison MIME execution recovery lock");
            })
            .join()
            .is_err()
        );

        let mut operation = block_on(group.try_submit_async(Default::default(), |_| async { Ok::<_, MimeError>(()) }))
            .expect("a no-op operation should be accepted");
        assert!(matches!(
            block_on(operation.wait_outcome()).unwrap(),
            qubit_operation::OperationOutcome::Succeeded(())
        ));
        assert!(block_on(operation.wait_settlement()).all_released());
        services.track_observer(operation.observer());
        services.track_recoveries(&[]);
        assert_eq!(0, services.unresolved_recovery_count());

        let shutdown = services.shutdown();
        assert!(
            block_on(shutdown.wait_services_settled())
                .expect("shutdown should recover poisoned tracking locks")
                .operations_settled
        );
        assert!(shutdown.stop_services().unwrap().services_stopped);
        assert_eq!(0, services.unresolved_recovery_count());
    }

    #[test]
    fn test_service_rejects_zero_cumulative_read_limit() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 1 }).unwrap();
        let result = MimeExecutionServices::new(manager, group, Arc::new(NoopPort), 0);
        assert!(matches!(result, Err(MimeError::InvalidOperationBudget)));
    }

    #[test]
    fn test_shutdown_observer_retries_failed_stop_after_settlement() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 1 }).unwrap();
        let port = Arc::new(RetryShutdownPort(AtomicUsize::new(0)));
        let services = MimeExecutionServices::new(manager, group, port.clone(), 32).unwrap();
        services.prepare().unwrap();

        let observer = services.shutdown();
        let initial = observer.snapshot();
        assert!(!initial.operations_settled);
        assert!(!initial.services_stopped);
        assert!(initial.group.is_none());
        assert!(matches!(
            block_on(observer.wait_group_shutdown()),
            Err(MimeError::OperationShutdownPending)
        ));
        assert!(matches!(
            observer.stop_services(),
            Err(MimeError::OperationShutdownPending)
        ));
        assert_eq!(port.0.load(Ordering::SeqCst), 0);

        let settled = block_on(observer.wait_services_settled()).unwrap();
        assert!(settled.operations_settled);
        assert!(!settled.services_stopped);
        assert!(settled.group.is_none());

        assert!(matches!(
            observer.stop_services(),
            Err(MimeError::ExecutionServicesRequired)
        ));
        let failed_stop = observer.snapshot();
        assert!(failed_stop.operations_settled);
        assert!(!failed_stop.services_stopped);
        assert!(failed_stop.group.is_none());

        let stopped = observer.stop_services().unwrap();
        assert!(stopped.operations_settled);
        assert!(stopped.services_stopped);
        assert!(stopped.group.is_some());
        assert!(observer.stop_services().unwrap().services_stopped);
        assert_eq!(port.0.load(Ordering::SeqCst), 2);
        let report = block_on(observer.wait_group_shutdown()).unwrap();
        assert_eq!(report.snapshot.active_operations, 0);
        assert_eq!(report.snapshot.queued_operations, 0);
    }
}
