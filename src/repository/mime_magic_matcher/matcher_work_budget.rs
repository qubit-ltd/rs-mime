// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Cooperative work accounting for recursive MIME magic matching.

/// Maximum number of matcher work units evaluated before yielding.
pub(crate) const MATCH_WORK_PER_BATCH: usize = 4096;

/// Tracks matcher work across repository rules and matcher nodes.
#[derive(Debug, Default)]
pub(crate) struct MatcherWorkBudget {
    /// Work counted since the most recent cooperative yield.
    work_in_poll: usize,
    /// Largest batch observed for the current matcher operation.
    maximum_work_in_poll: usize,
    /// Total work units charged across the operation.
    total_work: usize,
    /// Number of cooperative yields triggered by the budget.
    yields: usize,
}

impl MatcherWorkBudget {
    /// Counts one unit of matching work and reports whether the batch is full.
    pub(crate) fn charge(&mut self) -> bool {
        self.work_in_poll += 1;
        self.total_work += 1;
        if self.work_in_poll == MATCH_WORK_PER_BATCH {
            self.maximum_work_in_poll = self.maximum_work_in_poll.max(self.work_in_poll);
            self.work_in_poll = 0;
            self.yields += 1;
            true
        } else {
            false
        }
    }

    /// Records the final partial batch after matching completes.
    pub(crate) fn finish(&mut self) {
        self.maximum_work_in_poll = self.maximum_work_in_poll.max(self.work_in_poll);
    }

    /// Returns the largest work batch observed by this matcher operation.
    #[cfg(test)]
    pub(crate) fn maximum_work_in_poll(&self) -> usize {
        self.maximum_work_in_poll
    }

    /// Returns total counted work units.
    #[cfg(test)]
    pub(crate) fn total_work(&self) -> usize {
        self.total_work
    }

    /// Returns the number of cooperative yields caused by matcher work.
    #[cfg(test)]
    pub(crate) fn yields(&self) -> usize {
        self.yields
    }
}
