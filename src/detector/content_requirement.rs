// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Input window declarations for MIME content backends.

/// Maximum input window requested by a content backend.
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
    /// Inspect at most the leading prefix of the stream. This is a read
    /// window, not a minimum length guarantee; the input may end sooner.
    Prefix(
        /// Maximum number of leading bytes to make available to the backend.
        usize,
    ),
    /// Inspect the complete stream.
    Complete,
}
