// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Tests for indexed MIME magic priority, ordering, and de-duplication.

use crate::MagicValueType;
use crate::MimeMagic;
use crate::MimeMagicMatcher;
use crate::repository::magic_index::MagicIndex;

fn rule(priority: u16, value: &[u8]) -> MimeMagic {
    let matcher = MimeMagicMatcher::new(MagicValueType::String, 0, 0, value.to_vec(), None, vec![])
        .expect("test matcher should be valid");
    MimeMagic::new(priority, vec![matcher]).expect("test magic rule should be valid")
}

#[test]
fn magic_index_returns_only_highest_priority_in_source_order_once_per_mime() {
    let mut index = MagicIndex::default();
    index.add(3, &rule(10, b"same"));
    index.add(7, &rule(40, b"same"));
    index.add(3, &rule(40, b"same"));
    index.add(9, &rule(40, b"same"));
    index.add(11, &rule(40, b"other"));

    assert_eq!(vec![7, 3, 9], index.matches(b"same"));
}

#[test]
fn magic_index_replaces_prior_results_and_returns_empty_for_no_match() {
    let mut index = MagicIndex::default();
    index.add(1, &rule(20, b"same"));
    index.add(2, &rule(80, b"same"));
    index.add(3, &rule(80, b"same"));

    assert_eq!(vec![2, 3], index.matches(b"same"));
    assert!(index.matches(b"miss").is_empty());
}
