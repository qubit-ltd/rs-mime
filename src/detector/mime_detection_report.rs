// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Owned candidate and report values returned by managed detection.

use super::mime_input::InspectedExtent;

/// A ranked MIME candidate. The rank is backend-defined and lower ranks win.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MimeCandidate {
    pub mime_type: String,
    pub rank: u32,
}

/// Candidate set and bounded input evidence produced by a detector backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MimeCandidates {
    /// Ordered candidates, best first.
    pub candidates: Vec<MimeCandidate>,
    /// Input region actually inspected.
    pub inspected_extent: InspectedExtent,
    /// Retained prefix, bounded by runtime policy.
    pub retained_prefix: Vec<u8>,
}

/// Summary of bytes retained and read while serving a request.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MimeBudgetSummary {
    /// Bytes charged to owned-memory budget at the report boundary.
    pub owned_bytes: usize,
    /// Bytes actually read from providers, including repeated reads.
    pub bytes_read: u64,
    /// Peak temporary storage charged to the request.
    pub staging_bytes: u64,
}

/// Owned business result; deliberately contains no operation resource handles.
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
