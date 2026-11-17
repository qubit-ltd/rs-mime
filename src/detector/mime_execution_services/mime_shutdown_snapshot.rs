// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Snapshot of MIME service and operation-group shutdown progress.

use qubit_operation::ShutdownSnapshot;

/// Snapshot separating execution-service shutdown from operation-group drain.
///
/// # Examples
///
/// ```
/// use qubit_mime::detector::mime_execution_services::MimeShutdownSnapshot;
///
/// let snapshot = MimeShutdownSnapshot {
///     operations_settled: true,
///     services_stopped: true,
///     unresolved_recoveries: 0,
///     group: None,
/// };
/// assert!(snapshot.operations_settled && snapshot.services_stopped);
/// ```
#[must_use = "inspect the shutdown progress before proceeding"]
#[derive(Debug, Clone)]
pub struct MimeShutdownSnapshot {
    /// Whether every tracked operation has published settlement.
    pub operations_settled: bool,
    /// Whether MIME workers have stopped after operation settlement.
    pub services_stopped: bool,
    /// Number of retained recovery handles that are not resolved or
    /// transferred.
    pub unresolved_recoveries: usize,
    /// Group drain progress, once the service port has stopped.
    pub group: Option<ShutdownSnapshot>,
}
