/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! MIME content magic rule.

use crate::MimeMagicMatcher;

/// A priority-ranked set of magic matchers for one MIME type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MimeMagic {
    priority: u16,
    matchers: Vec<MimeMagicMatcher>,
}

impl MimeMagic {
    /// Minimum valid magic priority.
    pub const MIN_PRIORITY: u16 = 0;
    /// Maximum valid magic priority.
    pub const MAX_PRIORITY: u16 = 100;
    /// Default magic priority.
    pub const DEFAULT_PRIORITY: u16 = 50;

    /// Creates a MIME magic rule.
    ///
    /// # Parameters
    /// - `priority`: Priority in `0..=100`.
    /// - `matchers`: Root matchers. Any root matcher may satisfy the magic rule.
    ///
    /// # Returns
    /// A new [`MimeMagic`].
    pub fn new(priority: u16, matchers: Vec<MimeMagicMatcher>) -> Self {
        Self { priority, matchers }
    }

    /// Gets this magic rule's priority.
    ///
    /// # Returns
    /// Magic priority used for conflict resolution.
    pub fn priority(&self) -> u16 {
        self.priority
    }

    /// Gets root matchers.
    ///
    /// # Returns
    /// Root matchers for this magic rule.
    pub fn matchers(&self) -> &[MimeMagicMatcher] {
        &self.matchers
    }

    /// Gets the maximum number of bytes needed by this magic rule.
    ///
    /// # Returns
    /// Highest byte count needed by any root matcher.
    pub fn max_test_bytes(&self) -> usize {
        self.matchers
            .iter()
            .map(MimeMagicMatcher::max_test_bytes)
            .max()
            .unwrap_or(0)
    }

    /// Tests whether this magic rule matches a content buffer.
    ///
    /// # Parameters
    /// - `bytes`: Content bytes to test.
    ///
    /// # Returns
    /// `true` when any root matcher matches.
    pub fn matches(&self, bytes: &[u8]) -> bool {
        self.matchers.iter().any(|matcher| matcher.matches(bytes))
    }
}
