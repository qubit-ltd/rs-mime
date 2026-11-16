// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Worker dispatch adapters for MIME command execution.

use std::sync::Arc;

use crate::detector::MimeExecutionPort;

mod fixed_mime_execution_port;
mod fixed_queue;
mod queue_state;
mod running_workers;

pub use fixed_mime_execution_port::FixedMimeExecutionPort;

/// Cloneable handle to the service-owned fixed-capacity worker dispatcher.
#[derive(Clone)]
pub(crate) struct MimeExecutionWorker {
    /// Execution port shared with the application runtime.
    port: Arc<dyn MimeExecutionPort>,
}

impl MimeExecutionWorker {
    /// Uses the dispatch service already owned by the application.
    ///
    /// # Parameters
    /// - `port`: Application-owned execution port.
    pub(crate) fn new(port: Arc<dyn MimeExecutionPort>) -> Self {
        Self { port }
    }

    /// Offers a job without blocking and returns ownership when rejected.
    ///
    /// # Parameters
    /// - `job`: Job offered to the execution port.
    ///
    /// # Returns
    /// `Ok(())` if accepted, or the original job if rejected.
    pub(crate) fn try_dispatch(
        &self,
        job: Box<dyn FnOnce() + Send + 'static>,
    ) -> Result<(), Box<dyn FnOnce() + Send + 'static>> {
        self.port.try_dispatch(job)
    }
}
