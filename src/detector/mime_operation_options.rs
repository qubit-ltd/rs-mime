// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Per-request policy and budget tightening options.

use qubit_operation::OperationOptions;

use super::mime_detection_policy::MimeDetectionPolicy;

/// Request-specific MIME policy. Values can tighten, but never enlarge,
/// execution-service limits.
///
/// # Examples
///
/// ```
/// use qubit_mime::MimeOperationOptions;
///
/// let options = MimeOperationOptions {
///     max_read_bytes: Some(128),
///     ..MimeOperationOptions::default()
/// };
/// assert_eq!(options.max_read_bytes, Some(128));
/// ```
#[must_use = "pass these options to a MIME operation submission"]
#[derive(Clone)]
pub struct MimeOperationOptions {
    /// Candidate resolution policy.
    pub policy: MimeDetectionPolicy,
    /// Maximum owned buffer bytes for this request, if further restricted.
    pub max_buffer_bytes: Option<usize>,
    /// Maximum staging bytes for this request, if further restricted.
    pub max_staging_bytes: Option<u64>,
    /// Maximum cumulative provider reads for this request, if further
    /// restricted.
    pub max_read_bytes: Option<u64>,
    /// Shared operation context, deadline, and stop options.
    pub operation: OperationOptions,
}

/// Supplies permissive per-request defaults that inherit service budgets.
impl Default for MimeOperationOptions {
    /// Creates default detection settings with no additional request limits.
    fn default() -> Self {
        Self {
            policy: MimeDetectionPolicy::default(),
            max_buffer_bytes: None,
            max_staging_bytes: None,
            max_read_bytes: None,
            operation: OperationOptions::default(),
        }
    }
}
