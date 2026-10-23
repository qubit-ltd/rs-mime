// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Indexed content magic matching.

use std::collections::HashSet;

use super::MimeMagic;
use super::internal::magic_entry::MagicEntry;

#[derive(Debug, Clone, Default)]
pub(crate) struct MagicIndex {
    /// Magic entries ordered by their source priority.
    pub(crate) entries: Vec<MagicEntry>,
    /// Largest byte offset needed by any indexed matcher.
    pub(crate) max_test_bytes: usize,
}

impl MagicIndex {
    /// Adds one MIME magic rule and updates the required input size.
    ///
    /// Entries are retained in insertion order so equal-priority MIME results
    /// follow repository order.
    ///
    /// # Parameters
    /// - `mime_index`: Position of the MIME type owning `magic` in the
    ///   repository's MIME type list.
    /// - `magic`: Rule to clone into the index.
    pub(crate) fn add(&mut self, mime_index: usize, magic: &MimeMagic) {
        self.max_test_bytes = self.max_test_bytes.max(magic.max_test_bytes());
        self.entries.push(MagicEntry {
            priority: magic.priority(),
            mime_index,
            magic: magic.clone(),
        });
    }

    /// Returns MIME indexes whose magic rules match the input bytes.
    ///
    /// Only entries at the highest matching priority are returned. A MIME
    /// index appears once even when multiple rules for that MIME type match;
    /// distinct results retain the order of their first matching entry.
    ///
    /// # Parameters
    /// - `bytes`: Content prefix available for evaluating the indexed rules.
    ///
    /// # Returns
    /// Distinct MIME indexes at the highest matching priority, or an empty
    /// vector when no rule matches.
    pub(crate) fn matches(&self, bytes: &[u8]) -> Vec<usize> {
        let Some(best_priority) = self
            .entries
            .iter()
            .filter(|entry| entry.magic.matches(bytes))
            .map(|entry| entry.priority)
            .max()
        else {
            return Vec::new();
        };
        let mut seen = HashSet::new();
        self.entries
            .iter()
            .filter(|entry| entry.priority == best_priority && entry.magic.matches(bytes))
            .filter_map(|entry| seen.insert(entry.mime_index).then_some(entry.mime_index))
            .collect()
    }
}
