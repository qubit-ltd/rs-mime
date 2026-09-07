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

#[derive(Debug, Clone)]
pub(crate) struct GlobEntry {
    pub(crate) glob: MimeGlob,
    pub(crate) mime_index: usize,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct GlobIndex {
    literals: HashMap<String, Vec<GlobEntry>>,
    extensions: HashMap<String, Vec<GlobEntry>>,
    wildcards: Vec<GlobEntry>,
}

impl GlobIndex {
    pub(crate) fn add(&mut self, mime_index: usize, glob: &MimeGlob) {
        let entry = GlobEntry {
            glob: glob.clone(),
            mime_index,
        };
        if let Some(extension) = extension_pattern(glob.pattern()) {
            self.extensions
                .entry(extension.to_ascii_lowercase())
                .or_default()
                .push(entry);
        } else if is_literal_pattern(glob.pattern()) {
            self.literals
                .entry(glob.pattern().to_ascii_lowercase())
                .or_default()
                .push(entry);
        } else {
            self.wildcards.push(entry);
        }
    }

    pub(crate) fn matches<'a>(&'a self, filename: &str) -> Vec<&'a GlobEntry> {
        let basename = filename.rsplit(['/', '\\']).next().unwrap_or_default();
        if basename.is_empty() {
            return Vec::new();
        }
        let folded = basename.to_ascii_lowercase();
        if let Some(entries) = self.literals.get(&folded) {
            let matching = entries.iter().filter(|entry| entry.glob.matches(basename));
            return deduplicate_by_mime(matching.collect());
        }
        let mut candidates = Vec::new();
        for extension in extension_suffixes(&folded) {
            if let Some(entries) = self.extensions.get(extension) {
                candidates.extend(entries.iter().filter(|entry| entry.glob.matches(basename)));
            }
        }
        candidates.extend(self.wildcards.iter().filter(|entry| entry.glob.matches(basename)));
        select_best(candidates)
    }
}

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

fn deduplicate_by_mime(entries: Vec<&GlobEntry>) -> Vec<&GlobEntry> {
    let mut seen = HashSet::new();
    entries
        .into_iter()
        .filter(|entry| seen.insert(entry.mime_index))
        .collect()
}

fn extension_suffixes(filename: &str) -> impl Iterator<Item = &str> {
    filename
        .match_indices('.')
        .map(|(index, _)| &filename[index + 1..])
        .filter(|extension| !extension.is_empty())
}

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

fn is_literal_pattern(pattern: &str) -> bool {
    !pattern
        .chars()
        .any(|ch| matches!(ch, '*' | '?' | '{' | '}' | '!' | '[' | ']' | '^'))
}
// qubit-style: allow multiple-public-types
