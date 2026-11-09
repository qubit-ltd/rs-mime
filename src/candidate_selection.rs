// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Shared candidate selection for filename and content MIME detection.

use crate::MimeDetectionPolicy;

/// Selects one candidate according to a MIME detection policy.
///
/// Filename order is kept when both sources agree. If they disagree, the
/// first content candidate wins. `VerifyContent` requires a content match,
/// while `PreferFilename` accepts a unique filename candidate immediately.
///
/// # Type Parameters
/// - `T`: Candidate type compared by the supplied equality function.
///
/// # Parameters
/// - `filename`: Candidates in filename precedence order.
/// - `content`: Candidates in content precedence order.
/// - `policy`: Rule for choosing between candidate sources.
/// - `same`: Predicate that identifies equivalent candidates.
///
/// # Returns
/// The selected candidate borrowed from either input, or `None` if neither
/// source provides an acceptable candidate for the policy.
pub(crate) fn select_candidate<'a, T>(
    filename: &'a [T],
    content: &'a [T],
    policy: MimeDetectionPolicy,
    same: impl Fn(&T, &T) -> bool,
) -> Option<&'a T> {
    if policy == MimeDetectionPolicy::VerifyContent && content.is_empty() {
        return None;
    }
    if filename.len() == 1 && policy == MimeDetectionPolicy::PreferFilename {
        return filename.first();
    }
    if filename.is_empty() {
        return content.first();
    }
    if content.is_empty() {
        return filename.first();
    }
    filename
        .iter()
        .find(|candidate| content.iter().any(|item| same(candidate, item)))
        .or_else(|| content.first())
}
