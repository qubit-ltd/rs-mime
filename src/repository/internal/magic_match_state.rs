// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Shared priority and deduplication state for synchronous and async matching.

use std::collections::HashSet;

/// Accumulates matching MIME indexes at the highest priority in source order.
#[derive(Debug, Default)]
pub(crate) struct MagicMatchState {
    /// Highest matching rule priority seen so far.
    best_priority: Option<u16>,
    /// Distinct matching MIME indexes in first-match source order.
    results: Vec<usize>,
    /// MIME indexes already added to `results` at the current priority.
    seen: HashSet<usize>,
}

impl MagicMatchState {
    /// Reports whether an entry below the current best priority cannot match.
    ///
    /// # Parameters
    /// - `priority`: Priority of the next ordered magic entry.
    ///
    /// # Returns
    /// `true` when a higher-priority match has already been recorded.
    #[must_use]
    pub(crate) fn skips(&self, priority: u16) -> bool {
        self.best_priority.is_some_and(|best| priority < best)
    }

    /// Adds a matching MIME index and resets lower-priority results as needed.
    ///
    /// Nonmatching entries leave the accumulated state unchanged. Callers skip
    /// lower-priority entries first; equal-priority matches retain source order
    /// and duplicate MIME indexes are ignored.
    ///
    /// # Parameters
    /// - `priority`: Priority of the evaluated magic entry.
    /// - `mime_index`: Repository index of the MIME type that matched.
    /// - `matches`: Whether the evaluated rule matched the input.
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

    /// Returns the accumulated MIME indexes in first-match order.
    #[must_use]
    pub(crate) fn into_results(self) -> Vec<usize> {
        self.results
    }
}
