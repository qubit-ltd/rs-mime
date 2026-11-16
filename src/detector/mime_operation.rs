// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Public typed handles for MIME detection operations.

use super::mime_detection_report::MimeDetectionReport;
use crate::MimeError;
use qubit_operation::{AsyncOperationHandle, OperationHandle};

/// Asynchronous operation handle for one MIME request.
pub type AsyncMimeOperation = AsyncOperationHandle<MimeDetectionReport, MimeError>;
/// Synchronous operation handle for one MIME request.
pub type MimeOperation = OperationHandle<MimeDetectionReport, MimeError>;

/// Local and manager admission reasons for a not-yet-started submission.
#[derive(Debug)]
pub enum MimeSubmitRejection {
    /// Admission rejected by operation's manager.
    Admission(qubit_operation::AdmissionError),
    /// No execution services were configured.
    ExecutionServicesRequired,
    /// Request options were invalid.
    InvalidOptions(String),
    /// A finite request budget would be exceeded.
    BudgetExceeded,
    /// The selected operation group could not be bound.
    GroupBinding(qubit_operation::GroupBindingError),
}

/// Failed submission with the original input returned to its caller.
pub struct MimeSubmitFailure {
    /// Reason the body was rejected before it started.
    pub reason: MimeSubmitRejection,
    /// Input returned intact for retry or alternate handling.
    pub input: super::mime_input::OwnedMimeInput,
}
