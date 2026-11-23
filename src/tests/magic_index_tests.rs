// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Tests for indexed MIME magic priority, ordering, and de-duplication.

use crate::MagicValueType;
use crate::MimeMagic;
use crate::MimeMagicMatcher;
use crate::MimeType;
use crate::repository::magic_index::MagicIndex;

/// Builds a string-valued magic rule for priority and ordering scenarios.
fn rule(priority: u16, value: &[u8]) -> MimeMagic {
    let matcher = MimeMagicMatcher::new(MagicValueType::String, 0, 0, value.to_vec(), None, vec![])
        .expect("test matcher should be valid");
    MimeMagic::new(priority, vec![matcher]).expect("test magic rule should be valid")
}

fn mime_types_with_rules(entries: &[(usize, u16, &[u8])]) -> Vec<MimeType> {
    let count = entries
        .iter()
        .map(|(index, _, _)| index + 1)
        .max()
        .unwrap_or(0);
    let mut magics = vec![Vec::new(); count];
    for (mime_index, priority, value) in entries {
        magics[*mime_index].push(rule(*priority, value));
    }
    magics
        .into_iter()
        .enumerate()
        .map(|(index, rules)| {
            let name = format!("application/x-{index}");
            rules
                .into_iter()
                .fold(MimeType::builder(&name), |builder, magic| {
                    builder.magic(magic)
                })
                .build()
                .expect("test MIME type should be valid")
        })
        .collect()
}

#[test]
fn test_magic_index_returns_only_highest_priority_in_source_order_once_per_mime() {
    let mime_types = mime_types_with_rules(&[
        (3, 10, b"same"),
        (7, 40, b"same"),
        (3, 40, b"same"),
        (9, 40, b"same"),
        (11, 40, b"other"),
    ]);
    let mut index = MagicIndex::default();
    for (mime_index, magic_index, priority, value) in [
        (3, 0, 10, b"same".as_slice()),
        (7, 0, 40, b"same".as_slice()),
        (3, 1, 40, b"same".as_slice()),
        (9, 0, 40, b"same".as_slice()),
        (11, 0, 40, b"other".as_slice()),
    ] {
        index.add(mime_index, magic_index, &rule(priority, value));
    }

    assert_eq!(vec![7, 3, 9], index.matches(&mime_types, b"same"));
}

#[test]
fn test_magic_index_replaces_prior_results_and_returns_empty_for_no_match() {
    let mime_types = mime_types_with_rules(&[(1, 20, b"same"), (2, 80, b"same"), (3, 80, b"same")]);
    let mut index = MagicIndex::default();
    index.add(1, 0, &rule(20, b"same"));
    index.add(2, 0, &rule(80, b"same"));
    index.add(3, 0, &rule(80, b"same"));

    assert_eq!(vec![2, 3], index.matches(&mime_types, b"same"));
    assert!(index.matches(&mime_types, b"miss").is_empty());
}
