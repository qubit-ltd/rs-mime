// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Shared classification for the glob syntax implemented by `MimeGlob`.

/// Returns whether a pattern contains glob syntax recognized by the matcher.
///
/// # Parameters
/// - `pattern`: Glob pattern to inspect.
///
/// # Returns
/// `true` when the pattern contains `*`, `?`, or `[`.
#[must_use]
pub(crate) fn has_pattern_syntax(pattern: &str) -> bool {
    pattern.chars().any(|ch| matches!(ch, '*' | '?' | '['))
}

/// Returns a non-empty simple extension from a `*.extension` pattern.
///
/// # Parameters
/// - `pattern`: Glob pattern to inspect.
///
/// # Returns
/// The extension when it is non-empty and contains no matcher syntax.
#[must_use]
pub(crate) fn simple_extension(pattern: &str) -> Option<&str> {
    let extension = pattern.strip_prefix("*.")?;
    (!extension.is_empty() && !has_pattern_syntax(extension)).then_some(extension)
}
