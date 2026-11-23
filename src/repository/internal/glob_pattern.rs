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
/// `true` when the pattern contains `*`, `?`, or a closed character class.
#[must_use]
pub(crate) fn has_pattern_syntax(pattern: &str) -> bool {
    let chars: Vec<_> = pattern.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        match chars[index] {
            '*' | '?' => return true,
            '[' => {
                if character_class_end(&chars, index).is_some() {
                    return true;
                }
                index += 1;
            }
            _ => index += 1,
        }
    }
    false
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

/// Finds the closing bracket for a character class using the matcher's glob
/// syntax rules.
pub(crate) fn character_class_end(chars: &[char], start: usize) -> Option<usize> {
    let mut content_start = start + 1;
    if chars.get(content_start) == Some(&'!') {
        content_start += 1;
    }
    let mut end = content_start;
    let empty_group = chars.get(content_start) == Some(&']') && chars.len() == content_start + 1;
    if !empty_group && chars.get(end) == Some(&']') {
        end += 1;
    }
    while end < chars.len() && chars[end] != ']' {
        end += 1;
    }
    (end < chars.len()).then_some(end)
}

#[cfg(test)]
mod tests {
    use super::has_pattern_syntax;
    use super::simple_extension;

    #[test]
    fn unclosed_character_class_is_literal_syntax() {
        assert!(!has_pattern_syntax("file[.txt"));
        assert_eq!(Some("txt[ab"), simple_extension("*.txt[ab"));
    }

    #[test]
    fn closed_character_class_remains_pattern_syntax() {
        assert!(has_pattern_syntax("file[ab].txt"));
        assert_eq!(None, simple_extension("*.txt[ab]"));
    }
}
