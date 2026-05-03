/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Filename glob rules for MIME detection.

use regex::{Regex, RegexBuilder};

use crate::{MimeError, MimeResult};

/// Filename glob rule associated with a MIME type.
#[derive(Debug, Clone)]
pub struct MimeGlob {
    weight: u16,
    case_sensitive: bool,
    pattern: String,
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
    /// [`MimeGlob::MAX_WEIGHT`].
    pub fn new(pattern: &str, weight: u16, case_sensitive: bool) -> MimeResult<Self> {
        if weight > Self::MAX_WEIGHT {
            return Err(MimeError::InvalidGlobWeight { weight });
        }
        let regex = glob_to_regex(pattern);
        let matcher = RegexBuilder::new(&regex)
            .case_insensitive(!case_sensitive)
            .build()
            .expect("generated MIME glob regex should be valid");
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
    pub fn weight(&self) -> u16 {
        self.weight
    }

    /// Tells whether this glob is case-sensitive.
    ///
    /// # Returns
    /// `true` if filename case must match exactly.
    pub fn case_sensitive(&self) -> bool {
        self.case_sensitive
    }

    /// Gets the original glob pattern.
    ///
    /// # Returns
    /// Pattern text from the MIME database.
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
    let mut end = start + 1;
    while end < chars.len() && chars[end] != ']' {
        end += 1;
    }
    if end >= chars.len() {
        regex.push_str("\\[");
        return start;
    }
    regex.push('[');
    let mut content_start = start + 1;
    if content_start < end && chars[content_start] == '!' {
        regex.push('^');
        content_start += 1;
    }
    for ch in chars.iter().take(end).skip(content_start) {
        if *ch == '\\' {
            regex.push('\\');
        }
        regex.push(*ch);
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
        '.' | '+'
            | '('
            | ')'
            | '|'
            | '^'
            | '$'
            | '{'
            | '}'
            | '='
            | '!'
            | '<'
            | '>'
            | ':'
            | '-'
            | '\\'
    ) {
        regex.push('\\');
    }
    regex.push(ch);
}

// qubit-style: allow coverage-cfg
#[cfg(coverage)]
pub(crate) mod coverage_support {
    //! Coverage helpers for glob conversion branches.

    use super::MimeGlob;

    /// Exercises glob parser edge cases not common in the default database.
    ///
    /// # Returns
    /// Match results encoded as strings.
    pub(crate) fn exercise_glob_edges() -> Vec<String> {
        let question = MimeGlob::new("file?.txt", 50, false).expect("glob should compile");
        let negated_class = MimeGlob::new("[!a]ile.txt", 50, false).expect("glob should compile");
        let unclosed_class = MimeGlob::new("file[.txt", 50, false).expect("glob should compile");
        let escaped_class = MimeGlob::new("[\\]a].txt", 50, false).expect("glob should compile");
        vec![
            question.matches("file1.txt").to_string(),
            negated_class.matches("bile.txt").to_string(),
            unclosed_class.matches("file[.txt").to_string(),
            escaped_class.matches("\\.txt").to_string(),
            question.case_sensitive().to_string(),
        ]
    }
}
