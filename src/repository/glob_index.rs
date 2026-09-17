// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Indexed filename glob matching.

use std::collections::HashMap;
use std::collections::HashSet;

use super::MimeGlob;
use super::internal::glob_entry::GlobEntry;

#[derive(Debug, Clone, Default)]
pub(crate) struct GlobIndex {
    /// Literal patterns indexed by their complete filename.
    literals: HashMap<String, Vec<GlobEntry>>,
    /// Literal patterns containing Unicode characters.
    unicode_literals: Vec<GlobEntry>,
    /// Extension patterns indexed by their suffix.
    extensions: HashMap<String, Vec<GlobEntry>>,
    /// Patterns requiring wildcard matching.
    wildcards: Vec<GlobEntry>,
}

impl GlobIndex {
    /// Adds one MIME glob to the appropriate lookup index.
    pub(crate) fn add(&mut self, mime_index: usize, glob: &MimeGlob) {
        let entry = GlobEntry {
            glob: glob.clone(),
            mime_index,
        };
        if let Some(extension) = extension_pattern(glob.pattern()) {
            if !extension.is_ascii() {
                self.wildcards.push(entry);
                return;
            }
            self.extensions
                .entry(extension.to_ascii_lowercase())
                .or_default()
                .push(entry);
        } else if is_literal_pattern(glob.pattern()) {
            if !glob.pattern().is_ascii() {
                self.unicode_literals.push(entry);
                return;
            }
            self.literals
                .entry(glob.pattern().to_ascii_lowercase())
                .or_default()
                .push(entry);
        } else {
            self.wildcards.push(entry);
        }
    }

    /// Returns the best matching entries for a filename.
    pub(crate) fn matches<'a>(&'a self, filename: &str) -> Vec<&'a GlobEntry> {
        let basename = filename.rsplit(['/', '\\']).next().unwrap_or_default();
        if basename.is_empty() {
            return Vec::new();
        }
        let folded = basename.to_ascii_lowercase();
        let mut literal_candidates = self
            .literals
            .get(&folded)
            .into_iter()
            .flat_map(|entries| entries.iter())
            .filter(|entry| entry.glob.matches(basename))
            .collect::<Vec<_>>();
        literal_candidates.extend(
            self.unicode_literals
                .iter()
                .filter(|entry| entry.glob.matches(basename)),
        );
        if !basename.is_ascii() {
            literal_candidates.extend(
                self.literals
                    .values()
                    .flat_map(|entries| entries.iter())
                    .filter(|entry| entry.glob.matches(basename)),
            );
        }
        let literal_candidates = deduplicate_by_mime(literal_candidates);
        if !literal_candidates.is_empty() {
            return literal_candidates;
        }
        let mut candidates = Vec::new();
        for extension in extension_suffixes(&folded) {
            if let Some(entries) = self.extensions.get(extension) {
                candidates.extend(entries.iter().filter(|entry| entry.glob.matches(basename)));
            }
        }
        if !basename.is_ascii() {
            candidates.extend(
                self.extensions
                    .values()
                    .flat_map(|entries| entries.iter())
                    .filter(|entry| entry.glob.matches(basename)),
            );
        }
        candidates.extend(self.wildcards.iter().filter(|entry| entry.glob.matches(basename)));
        select_best(candidates)
    }
}

/// Selects entries with the highest glob weight and longest pattern.
fn select_best(entries: Vec<&GlobEntry>) -> Vec<&GlobEntry> {
    let Some(best_weight) = entries.iter().map(|entry| entry.glob.weight()).max() else {
        return Vec::new();
    };
    let best_weight: Vec<_> = entries
        .into_iter()
        .filter(|entry| entry.glob.weight() == best_weight)
        .collect();
    let best_length = best_weight
        .iter()
        .map(|entry| entry.glob.pattern().len())
        .max()
        .unwrap_or_default();
    deduplicate_by_mime(
        best_weight
            .into_iter()
            .filter(|entry| entry.glob.pattern().len() == best_length)
            .collect(),
    )
}

/// Removes duplicate MIME indexes while preserving match order.
fn deduplicate_by_mime(entries: Vec<&GlobEntry>) -> Vec<&GlobEntry> {
    let mut seen = HashSet::new();
    entries
        .into_iter()
        .filter(|entry| seen.insert(entry.mime_index))
        .collect()
}

/// Returns non-empty suffixes after each dot in a filename.
fn extension_suffixes(filename: &str) -> impl Iterator<Item = &str> {
    filename
        .match_indices('.')
        .map(|(index, _)| &filename[index + 1..])
        .filter(|extension| !extension.is_empty())
}

/// Extracts a valid extension from a `*.extension` glob pattern.
fn extension_pattern(pattern: &str) -> Option<&str> {
    let extension = pattern.strip_prefix("*.")?;
    if extension.is_empty()
        || extension
            .chars()
            .any(|ch| matches!(ch, '*' | '?' | '{' | '}' | '!' | '[' | ']' | '^'))
    {
        None
    } else {
        Some(extension)
    }
}

/// Returns whether a pattern contains no glob metacharacters.
fn is_literal_pattern(pattern: &str) -> bool {
    !pattern
        .chars()
        .any(|ch| matches!(ch, '*' | '?' | '{' | '}' | '!' | '[' | ']' | '^'))
}
