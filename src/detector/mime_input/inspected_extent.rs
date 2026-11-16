// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Extent that a MIME backend actually inspected.

/// Portion of an input actually inspected by a backend.
///
/// # Variants
/// - `Prefix`: A contiguous prefix measured in bytes.
/// - `Complete`: The complete input was inspected.
/// - `RandomAccessSamples`: Noncontiguous samples were inspected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InspectedExtent {
    /// A contiguous prefix whose length is recorded in bytes.
    Prefix(#[doc = "Number of contiguous prefix bytes inspected."] usize),
    /// The complete input was inspected.
    Complete,
    /// The backend inspected samples through random access.
    RandomAccessSamples,
}
