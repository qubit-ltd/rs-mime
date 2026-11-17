// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Completeness claim attached to MIME byte input.

/// Completeness claim attached to byte input.
///
/// # Examples
///
/// ```
/// use qubit_mime::InputExtent;
///
/// let extent = InputExtent::Complete;
/// assert_eq!(extent, InputExtent::Complete);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputExtent {
    /// The bytes represent only the beginning of a resource.
    Prefix,
    /// The bytes represent the complete resource.
    Complete,
}
