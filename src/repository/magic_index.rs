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
use super::MimeType;
use super::internal::magic_entry::MagicEntry;

/// Accumulates the highest-priority matching MIME indexes in source order.
#[derive(Debug, Default)]
pub(crate) struct MagicMatchState {
    best_priority: Option<u16>,
    results: Vec<usize>,
    seen: HashSet<usize>,
}

impl MagicMatchState {
    /// Returns whether entries below `priority` can be skipped.
    #[must_use]
    pub(crate) fn skips(&self, priority: u16) -> bool {
        self.best_priority.is_some_and(|best| priority < best)
    }

    /// Records a matching rule while preserving priority and source order.
    pub(crate) fn record(&mut self, priority: u16, mime_index: usize, matches: bool) {
        if !matches {
            return;
        }
        match self.best_priority {
            Some(best) if priority > best => {
                self.best_priority = Some(priority);
                self.results.clear();
                self.seen.clear();
            }
            None => self.best_priority = Some(priority),
            _ => {}
        }
        if self.seen.insert(mime_index) {
            self.results.push(mime_index);
        }
    }

    /// Returns accumulated MIME indexes.
    #[must_use]
    pub(crate) fn into_results(self) -> Vec<usize> {
        self.results
    }
}

/// Indexes content magic rules and tracks their maximum required input size.
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
    pub(crate) fn add(&mut self, mime_index: usize, magic_index: usize, magic: &MimeMagic) {
        self.max_test_bytes = self.max_test_bytes.max(magic.max_test_bytes());
        self.entries.push(MagicEntry {
            priority: magic.priority(),
            mime_index,
            magic_index,
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
    #[must_use]
    pub(crate) fn matches(&self, mime_types: &[MimeType], bytes: &[u8]) -> Vec<usize> {
        let mut state = MagicMatchState::default();
        for entry in &self.entries {
            if state.skips(entry.priority) {
                continue;
            }
            let matched = mime_types
                .get(entry.mime_index)
                .and_then(|mime| mime.magics().get(entry.magic_index))
                .is_some_and(|magic| magic.matches(bytes));
            state.record(entry.priority, entry.mime_index, matched);
        }
        state.into_results()
    }
}
