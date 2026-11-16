// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Owned candidate and report values returned by managed detection.

mod mime_budget_summary;
mod mime_candidate;
mod mime_candidates;
mod mime_detection_report;

pub use mime_budget_summary::MimeBudgetSummary;
pub use mime_candidate::MimeCandidate;
pub use mime_candidates::MimeCandidates;
pub use mime_detection_report::MimeDetectionReport;
