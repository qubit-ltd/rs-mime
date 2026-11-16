// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Per-request staging cap and shared execution resources.

use crate::command_execution::MimeExecutionWorker;
use crate::detector::StagingBudget;

/// Per-request staging cap and the shared service accounting handle.
#[derive(Clone)]
pub(crate) struct ManagedStagingLimits {
    /// Maximum staged bytes for this request.
    pub(crate) max_bytes: u64,
    /// Shared budget used to account staged and owned bytes.
    pub(crate) budget: StagingBudget,
    /// Worker used for filesystem operations.
    pub(crate) worker: MimeExecutionWorker,
}
