// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Indexed content magic matching.

use super::MimeMagic;
use super::MimeType;
use super::internal::magic_entry::MagicEntry;
use super::internal::magic_match_state::MagicMatchState;

/// Indexes content magic rules and tracks their maximum required input size.
#[derive(Debug, Clone, Default)]
pub(crate) struct MagicIndex {
    /// Magic entries in repository MIME-type order, then each type's rule order.
    pub(crate) entries: Vec<MagicEntry>,
    /// Largest byte offset needed by any indexed matcher.
    pub(crate) max_test_bytes: usize,
}

impl MagicIndex {
    /// Adds one MIME magic rule and updates the required input size.
    ///
    /// Entries are retained in insertion order. Equal-priority results follow
    /// the first matching rule's insertion order, and repeated matches for a
    /// MIME type produce only one result.
    ///
    /// # Parameters
    /// - `mime_index`: Position of the MIME type owning `magic` in the
    ///   repository's MIME type list.
    /// - `magic_index`: Position of `magic` in its owning MIME type's rule
    ///   list.
    /// - `magic`: Rule whose priority and required input size are indexed.
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
    /// - `mime_types`: Repository MIME types, indexed by each entry's
    ///   `mime_index`.
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
