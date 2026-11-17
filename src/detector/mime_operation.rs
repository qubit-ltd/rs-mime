// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Public typed handles and submission errors for MIME detection operations.

use qubit_operation::AsyncOperationHandle;
use qubit_operation::OperationHandle;

use super::mime_detection_report::MimeDetectionReport;
use crate::MimeError;

mod mime_submit_failure;
mod mime_submit_rejection;

pub use mime_submit_failure::MimeSubmitFailure;
pub use mime_submit_rejection::MimeSubmitRejection;

/// Asynchronous operation handle for one MIME request.
///
/// The runtime returns this handle from asynchronous submission methods.
///
/// # Examples
///
/// ```
/// use qubit_mime::AsyncMimeOperation;
///
/// fn log_operation_id(operation: &AsyncMimeOperation) {
///     println!("{:?}", operation.id());
/// }
/// ```
pub type AsyncMimeOperation = AsyncOperationHandle<MimeDetectionReport, MimeError>;
/// Synchronous operation handle for one MIME request.
///
/// The runtime returns this handle from synchronous submission methods.
///
/// # Examples
///
/// ```
/// use qubit_mime::MimeOperation;
///
/// fn log_operation_id(operation: &MimeOperation) {
///     println!("{:?}", operation.id());
/// }
/// ```
pub type MimeOperation = OperationHandle<MimeDetectionReport, MimeError>;
