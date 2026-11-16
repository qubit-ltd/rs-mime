// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================

use super::super::mime_input::InspectedExtent;
use super::mime_candidate::MimeCandidate;

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
