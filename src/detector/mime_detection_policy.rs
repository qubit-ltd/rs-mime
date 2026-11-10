// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Policy for resolving combined MIME detection from filename and content.

/// Policy for resolving combined MIME detection from filename and content.
///
/// # Examples
///
/// ```
/// use qubit_mime::MimeDetectionPolicy;
/// assert_eq!(MimeDetectionPolicy::default(), MimeDetectionPolicy::Freedesktop);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MimeDetectionPolicy {
    /// Follow freedesktop shared-mime-info precedence rules. When filename
    /// and content results differ, a filename MIME type that inherits from the
    /// content MIME type is preferred. Repository-backed detectors use their
    /// own alias and inheritance data; standalone detector cores use the
    /// bundled database for this comparison.
    #[default]
    Freedesktop,

    /// Always evaluate content and use it to resolve filename ambiguity.
    ContentFirst,

    /// Prefer a definitive filename result without checking content magic.
    PreferFilename,

    /// Check content magic even when filename detection has a definitive
    /// result, and require a content match. No filename fallback is returned
    /// when content detection finds no candidate; use [`Self::ContentFirst`]
    /// to retain that fallback.
    VerifyContent,
}
