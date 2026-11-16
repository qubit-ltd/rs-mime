// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! MIME-owned ports and shared operation context for managed execution.

use std::fmt::Debug;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU8, Ordering},
};

use super::internal::mime_budget::{
    BudgetPermit, Limits, MimeBudget, ReadTreeContext, ReadTreeRegistry, StagingBudget,
};
use qubit_operation::{
    AsyncOperationGroup, AsyncOperationManager, AsyncOperationObserver, AsyncShutdownHandle,
    OperationGroup, OperationManager, ShutdownReport, ShutdownSnapshot,
};

/// MIME-specific command and I/O execution boundary. Implementations are
/// MIME-owned adapters; their callbacks must preserve typed domain errors.
pub trait MimeExecutionPort: Debug + Send + Sync + 'static {
    /// Explicitly starts fixed-capacity workers, if required.
    fn prepare(&self) -> Result<(), crate::MimeError>;
    /// Tries to dispatch one owned command or I/O job without blocking.
    /// Returning the job on rejection preserves captured resources.
    fn try_dispatch(
        &self,
        job: Box<dyn FnOnce() + Send + 'static>,
    ) -> Result<(), Box<dyn FnOnce() + Send + 'static>> {
        Err(job)
    }
    /// Stops accepting new work and observes actual worker exit.
    fn shutdown(&self) -> Result<(), crate::MimeError>;
}

/// Services shared by MIME requests. Runtime construction is explicit so a
/// closed service cannot silently create replacement workers.
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
    pub execution: Arc<dyn MimeExecutionPort>,
    /// Finite cumulative bytes-read limit required by construction.
    pub max_read_bytes: u64,
    budget: Arc<MimeBudget>,
    read_trees: Arc<ReadTreeRegistry>,
    lifecycle: Arc<AtomicU8>,
    top_closed: Arc<AtomicBool>,
    lifecycle_lock: Arc<Mutex<()>>,
    submission_lock: Arc<Mutex<()>>,
    observers: Arc<Mutex<Vec<AsyncOperationObserver>>>,
    group_shutdown: Arc<Mutex<Option<AsyncShutdownHandle>>>,
    operations_settled: Arc<AtomicBool>,
}

/// Repeatable MIME shutdown observer. Service settlement is independent of
/// caller outcome disposal; group shutdown can remain pending afterward.
#[derive(Clone)]
pub struct MimeShutdownObserver {
    services: MimeExecutionServices,
}

/// Snapshot separating execution-service shutdown from operation-group drain.
#[derive(Debug, Clone)]
pub struct MimeShutdownSnapshot {
    /// Whether every tracked operation has published settlement.
    pub operations_settled: bool,
    /// Whether MIME workers have stopped after operation settlement.
    pub services_stopped: bool,
    /// Group drain progress, once the service port has stopped.
    pub group: Option<ShutdownSnapshot>,
}

impl MimeExecutionServices {
    /// Creates services with a finite nonzero cumulative read limit.
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
            lifecycle_lock: Arc::new(Mutex::new(())),
            submission_lock: Arc::new(Mutex::new(())),
            observers: Arc::new(Mutex::new(Vec::new())),
            group_shutdown: Arc::new(Mutex::new(None)),
            operations_settled: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Starts MIME-owned fixed-capacity workers explicitly.
    pub fn prepare(&self) -> Result<(), crate::MimeError> {
        let _guard = self
            .lifecycle_lock
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        match self
            .lifecycle
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(_) => match self.execution.prepare() {
                Ok(()) => Ok(()),
                Err(error) => {
                    self.lifecycle.store(0, Ordering::Release);
                    Err(error)
                }
            },
            Err(1) => Ok(()),
            Err(_) => Err(crate::MimeError::ExecutionServicesRequired),
        }
    }

    /// Closes new top-level MIME submissions and returns a repeatable observer.
    /// Await operation settlements, call `stop_services` from an external
    /// synchronous context, then observe group Drain separately.
    pub fn shutdown(&self) -> MimeShutdownObserver {
        let _guard = self
            .lifecycle_lock
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if self.lifecycle.load(Ordering::Acquire) != 3 {
            self.top_closed.store(true, Ordering::Release);
            self.lifecycle.store(2, Ordering::Release);
        }
        MimeShutdownObserver {
            services: self.clone(),
        }
    }

    pub(in crate::detector) fn submission_guard(&self) -> std::sync::MutexGuard<'_, ()> {
        self.submission_lock
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    pub(in crate::detector) fn track_observer(&self, observer: AsyncOperationObserver) {
        let mut observers = self.observers.lock().unwrap_or_else(|e| e.into_inner());
        observers.retain(|item| item.snapshot().settlement.is_none());
        observers.push(observer);
    }

    pub(in crate::detector) fn is_prepared(&self) -> bool {
        self.lifecycle.load(Ordering::Acquire) == 1 && !self.top_closed.load(Ordering::Acquire)
    }
    pub(in crate::detector) fn accepts_children(&self) -> bool {
        match self.lifecycle.load(Ordering::Acquire) {
            1 => true,
            2 => !self.operations_settled.load(Ordering::Acquire),
            // Let the manager return a typed GroupBinding::Closed rejection
            // after Drain has closed the MIME group; never submit in this state.
            3 => true,
            _ => false,
        }
    }
    pub(in crate::detector) fn reserve_owned(&self, bytes: usize) -> Option<BudgetPermit> {
        self.budget.reserve_owned(bytes)
    }

    pub(in crate::detector) fn read_tree_for_parent(
        &self,
        parent: qubit_operation::OperationId,
        tree_limit: u64,
        scope_limit: u64,
    ) -> Arc<ReadTreeContext> {
        self.read_trees.child_for_parent(parent, tree_limit, scope_limit)
    }

    pub(in crate::detector) fn new_read_tree(&self, limit: u64) -> Arc<ReadTreeContext> {
        ReadTreeContext::new(limit)
    }

    pub(in crate::detector) fn bind_read_tree(
        &self,
        operation: qubit_operation::OperationId,
        tree: &Arc<ReadTreeContext>,
    ) {
        self.read_trees.bind(operation, tree);
    }
    pub(in crate::detector) fn reserve_allocation(
        &self,
        bytes: &Arc<[u8]>,
    ) -> Option<BudgetPermit> {
        self.budget.reserve_allocation(bytes)
    }
    #[allow(dead_code)]
    pub(in crate::detector) fn reserve_staging(&self, bytes: u64) -> Option<BudgetPermit> {
        self.budget.reserve_staging(bytes)
    }

    pub(crate) fn staging_budget(&self) -> StagingBudget {
        StagingBudget::from_budget(Arc::clone(&self.budget))
    }
}

impl MimeShutdownObserver {
    /// Waits asynchronously for every tracked operation settlement, then stops
    /// MIME workers and requests Drain for the MIME group. This does not wait
    /// for caller-owned typed handles to consume or discard their outcomes.
    pub async fn wait_services_settled(&self) -> Result<MimeShutdownSnapshot, crate::MimeError> {
        loop {
            let pending = self
                .services
                .observers
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            for observer in pending {
                if observer.snapshot().settlement.is_none() {
                    let _report = observer.wait_settlement().await;
                }
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

    /// Stops the MIME execution port after settlement. Call from outside MIME
    /// operation bodies and async polls because the adapter may block waiting
    /// for its fixed workers to exit. This is idempotent.
    pub fn stop_services(&self) -> Result<MimeShutdownSnapshot, crate::MimeError> {
        if !self.services.operations_settled.load(Ordering::Acquire) {
            return Err(crate::MimeError::OperationShutdownPending);
        }
        let _lifecycle = self
            .services
            .lifecycle_lock
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if self.services.lifecycle.load(Ordering::Acquire) != 3 {
            let _submissions = self.services.submission_guard();
            self.services.execution.shutdown()?;
            let shutdown = self
                .services
                .root_group
                .request_shutdown(qubit_operation::ShutdownMode::Drain)
                .as_async();
            *self
                .services
                .group_shutdown
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = Some(shutdown);
            self.services.lifecycle.store(3, Ordering::Release);
        }
        Ok(self.snapshot())
    }

    /// Returns service and group shutdown progress without waiting.
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
            group: handle.map(|value| value.snapshot()),
        }
    }

    /// Waits for group shutdown after workers and tracked operation settlements
    /// are complete. This may remain pending while callers retain typed handles.
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
