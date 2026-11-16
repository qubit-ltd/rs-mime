// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! MIME-owned ports and shared operation context for managed execution.

use std::fmt::Debug;
use std::sync::Arc;

use qubit_operation::{
    AsyncOperationGroup, AsyncOperationManager, OperationGroup, OperationManager,
};

/// MIME-specific command and I/O execution boundary. Implementations are
/// MIME-owned adapters; their callbacks must preserve typed domain errors.
pub trait MimeExecutionPort: Debug + Send + Sync + 'static {
    /// Explicitly starts fixed-capacity workers, if required.
    fn prepare(&self) -> Result<(), crate::MimeError>;
    /// Stops accepting new work and observes actual worker exit.
    fn shutdown(&self) -> Result<(), crate::MimeError>;
}

/// Services shared by MIME requests. Runtime construction is explicit so a
/// closed service cannot silently create replacement workers.
#[derive(Clone)]
pub struct MimeExecutionServices {
    /// Application-shared asynchronous manager.
    pub manager: AsyncOperationManager,
    /// Synchronous view of the shared manager, used by the sync facade.
    pub sync_manager: OperationManager,
    /// MIME's unparented root group and its async submission view.
    pub root_group: OperationGroup,
    /// Async view used by MIME async requests.
    pub async_root_group: AsyncOperationGroup,
    /// MIME-specific execution adapters.
    pub execution: Arc<dyn MimeExecutionPort>,
    /// Finite cumulative bytes-read limit required by construction.
    pub max_read_bytes: u64,
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
        })
    }
}
