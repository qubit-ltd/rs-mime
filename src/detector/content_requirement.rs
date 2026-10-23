// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Input size requirements for MIME content backends.

/// Amount of input a content backend must inspect.
///
/// # Examples
///
/// ```
/// use qubit_mime::ContentRequirement;
/// let requirement = ContentRequirement::Prefix(512);
/// match requirement {
///     ContentRequirement::Prefix(limit) => assert_eq!(limit, 512),
///     ContentRequirement::Complete => panic!("expected a prefix requirement"),
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentRequirement {
    /// Inspect only the leading prefix of the stream.
    Prefix(usize),
    /// Inspect the complete stream.
    Complete,
}
