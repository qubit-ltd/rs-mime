// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use super::super::mime_input::InspectedExtent;
use super::mime_budget_summary::MimeBudgetSummary;

/// Owned business result; deliberately contains no operation resource handles.
///
/// # Examples
///
/// ```
/// use qubit_mime::{InspectedExtent, MimeBudgetSummary, MimeDetectionReport};
///
/// let report = MimeDetectionReport {
///     mime_type: Some("application/pdf".to_owned()),
///     backend_id: "magic".to_owned(),
///     inspected_extent: InspectedExtent::Prefix(512),
///     budget: MimeBudgetSummary {
///         owned_bytes: 512,
///         bytes_read: 512,
///         staging_bytes: 0,
///     },
/// };
/// assert_eq!(report.mime_type.as_deref(), Some("application/pdf"));
/// assert_eq!(report.budget.bytes_read, 512);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MimeDetectionReport {
    /// Selected MIME type, if any candidate was selected.
    pub mime_type: Option<String>,
    /// Stable identifier of the backend that produced the selection.
    pub backend_id: String,
    /// Input region actually inspected.
    pub inspected_extent: InspectedExtent,
    /// Bounded usage summary.
    pub budget: MimeBudgetSummary,
}
