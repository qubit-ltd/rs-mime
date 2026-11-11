// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Filename glob rules for MIME detection.

use regex::Regex;
use regex::RegexBuilder;

use crate::MimeError;
use crate::MimeResult;

/// Filename glob rule associated with a MIME type.
///
/// # Examples
///
/// ```
/// use qubit_mime::MimeGlob;
/// let glob = MimeGlob::new("*.pdf", MimeGlob::DEFAULT_WEIGHT, false)?;
/// assert!(glob.matches("report.pdf"));
/// # Ok::<(), qubit_mime::MimeError>(())
/// ```
#[derive(Debug, Clone)]
pub struct MimeGlob {
    /// Weight used to rank matches against other filename globs.
    weight: u16,
    /// Whether matching preserves the configured case distinction.
    case_sensitive: bool,
    /// Original freedesktop-style glob pattern.
    pattern: String,
    /// Compiled whole-filename matcher generated from `pattern`.
    matcher: Regex,
}

impl MimeGlob {
    /// Minimum valid glob weight.
    pub const MIN_WEIGHT: u16 = 0;
    /// Maximum valid glob weight.
    pub const MAX_WEIGHT: u16 = 100;
    /// Default glob weight used by freedesktop shared MIME info.
    pub const DEFAULT_WEIGHT: u16 = 50;

    /// Creates a filename glob.
    ///
    /// # Parameters
    /// - `pattern`: Glob pattern from the MIME database.
    /// - `weight`: Match weight in the inclusive range `0..=100`.
    /// - `case_sensitive`: Whether matching should be case-sensitive.
    ///
    /// # Returns
    /// A compiled [`MimeGlob`].
    ///
    /// # Errors
    /// Returns [`MimeError::InvalidGlobWeight`](crate::MimeError::InvalidGlobWeight) when `weight` is greater than
    /// [`MimeGlob::MAX_WEIGHT`], or
    /// [`MimeError::InvalidXmlAttribute`](crate::MimeError::InvalidXmlAttribute)
    /// when the pattern is invalid or its regular expression exceeds the
    /// compiler's configured limits.
    pub fn new(pattern: &str, weight: u16, case_sensitive: bool) -> MimeResult<Self> {
        if weight > Self::MAX_WEIGHT {
            return Err(MimeError::InvalidGlobWeight { weight });
        }
        let regex = glob_to_regex(pattern);
        let matcher = RegexBuilder::new(&regex)
            .case_insensitive(!case_sensitive)
            .dot_matches_new_line(true)
            .build()
            .map_err(|error| MimeError::invalid_attr("glob", "pattern", pattern, error.to_string()))?;
        Ok(Self {
            weight,
            case_sensitive,
            pattern: pattern.to_owned(),
            matcher,
        })
    }

    /// Gets the glob weight.
    ///
    /// # Returns
    /// Glob weight used for conflict resolution.
    #[must_use]
    #[inline]
    pub fn weight(&self) -> u16 {
        self.weight
    }

    /// Tells whether this glob is case-sensitive.
    ///
    /// # Returns
    /// `true` if filename case must match exactly.
    #[must_use]
    #[inline]
    pub fn case_sensitive(&self) -> bool {
        self.case_sensitive
    }

    /// Gets the original glob pattern.
    ///
    /// # Returns
    /// Pattern text from the MIME database.
    #[must_use]
    #[inline]
    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    /// Tests whether a filename matches this glob.
    ///
    /// # Parameters
    /// - `filename`: Basename to match. Empty strings never match.
    ///
    /// # Returns
    /// `true` when the glob matches `filename`.
    #[must_use]
    #[inline]
    pub fn matches(&self, filename: &str) -> bool {
        !filename.is_empty() && !self.pattern.is_empty() && self.matcher.is_match(filename)
    }
}

/// Converts a MIME glob pattern into a whole-string regular expression.
///
/// # Parameters
/// - `pattern`: Freedesktop-style glob pattern.
///
/// # Returns
/// Regex text anchored at both ends.
#[must_use]
fn glob_to_regex(pattern: &str) -> String {
    let chars: Vec<char> = pattern.chars().collect();
    let mut regex = String::from("^");
    let mut index = 0;
    while index < chars.len() {
        match chars[index] {
            '*' => regex.push_str(".*"),
            '?' => regex.push('.'),
            '[' => index = append_character_class(&chars, index, &mut regex),
            ch => append_escaped_regex_char(ch, &mut regex),
        }
        index += 1;
    }
    regex.push('$');
    regex
}

/// Appends a glob character class to a regex.
///
/// # Parameters
/// - `chars`: Glob pattern characters.
/// - `start`: Index of the opening `[` character.
/// - `regex`: Destination regex text.
///
/// # Returns
/// Index of the closing `]` when a class was parsed; otherwise `start`.
fn append_character_class(chars: &[char], start: usize, regex: &mut String) -> usize {
    let mut content_start = start + 1;
    let negated = chars.get(content_start) == Some(&'!');
    if negated {
        content_start += 1;
    }
    let mut end = content_start;
    let empty_group = chars.get(content_start) == Some(&']') && (chars.len() == content_start + 1);
    if !empty_group && chars.get(end) == Some(&']') {
        end += 1;
    }
    while end < chars.len() && chars[end] != ']' {
        end += 1;
    }
    if end >= chars.len() {
        regex.push_str("\\[");
        return start;
    }
    regex.push('[');
    if negated {
        regex.push('^');
    }
    let mut index = content_start;
    while index < end {
        let ch = chars[index];
        // Regex character classes support set intersection, symmetric
        // difference, and difference operators. Escape their punctuation so
        // each source glob character remains literal. A single hyphen still
        // denotes a glob range; only the regex `--` operator is escaped.
        if matches!(ch, '[' | ']' | '\\' | '&' | '~' | '^') {
            regex.push('\\');
            regex.push(ch);
        } else if ch == '-' && (chars.get(index + 1) == Some(&'-') || chars.get(index.wrapping_sub(1)) == Some(&'-')) {
            regex.push_str("\\-");
        } else {
            regex.push(ch);
        }
        index += 1;
    }
    regex.push(']');
    end
}

/// Appends a literal regex character with escaping when needed.
///
/// # Parameters
/// - `ch`: Glob literal character.
/// - `regex`: Destination regex text.
fn append_escaped_regex_char(ch: char, regex: &mut String) {
    if matches!(
        ch,
        '.' | '+' | '(' | ')' | '|' | '^' | '$' | '{' | '}' | '=' | '!' | '<' | '>' | ':' | '-' | '\\'
    ) {
        regex.push('\\');
    }
    regex.push(ch);
}
