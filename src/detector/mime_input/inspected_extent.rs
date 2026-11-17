// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Extent that a MIME backend actually inspected.

/// Content range supplied to a backend's matching operation.
///
/// `Prefix(n)` records the bounded contiguous window made available to
/// matching. It is not a count of individual bytes read or touched by each
/// rule. Use [`super::InputExtent`] to describe whether the caller supplied a
/// prefix or the complete input.
///
/// # Variants
/// - `Prefix`: A contiguous prefix measured in bytes.
/// - `Complete`: The complete input was inspected.
/// - `RandomAccessSamples`: Noncontiguous samples were inspected.
///
/// # Examples
///
/// ```
/// use qubit_mime::InspectedExtent;
///
/// let inspected = InspectedExtent::Prefix(4096);
/// assert_eq!(inspected, InspectedExtent::Prefix(4096));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InspectedExtent {
    /// A bounded contiguous prefix made available to content matching.
    Prefix(#[doc = "Length of the contiguous matching window in bytes."] usize),
    /// The complete input was inspected.
    Complete,
    /// The backend inspected samples through random access.
    RandomAccessSamples,
}
