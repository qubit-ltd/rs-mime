// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use super::super::mime_input::InspectedExtent;
use super::mime_candidate::MimeCandidate;

/// Candidate set and bounded input evidence produced by a detector backend.
///
/// # Examples
///
/// ```
/// use qubit_mime::InspectedExtent;
/// use qubit_mime::MimeCandidate;
/// use qubit_mime::MimeCandidates;
///
/// let candidates = MimeCandidates {
///     candidates: vec![MimeCandidate {
///         mime_type: "application/pdf".to_owned(),
///         rank: 0,
///     }],
///     inspected_extent: InspectedExtent::Prefix(512),
///     retained_prefix: b"%PDF".to_vec(),
///     staging_bytes: 64,
/// };
/// assert_eq!(candidates.candidates[0].mime_type, "application/pdf");
/// assert_eq!(candidates.inspected_extent, InspectedExtent::Prefix(512));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MimeCandidates {
    /// Ordered candidates, best first.
    pub candidates: Vec<MimeCandidate>,
    /// Input region actually inspected.
    pub inspected_extent: InspectedExtent,
    /// Retained prefix, bounded by runtime policy.
    pub retained_prefix: Vec<u8>,
    /// Peak temporary storage charged while detecting this input.
    pub staging_bytes: u64,
}
