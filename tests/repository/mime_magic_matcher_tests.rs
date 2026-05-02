/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Tests for MIME magic matcher rules.

use qubit_mime::{MagicValueType, MimeError, MimeMagicMatcher};

#[test]
fn test_new_rejects_inverted_offset_range() {
    let error = MimeMagicMatcher::new(MagicValueType::String, 5, 4, b"ABC".to_vec(), None, vec![])
        .expect_err("inverted offset range should fail");

    assert!(matches!(
        error,
        MimeError::InvalidMagicMatcher { ref reason } if reason.contains("offset")
    ));
}

#[test]
fn test_matches_string_at_fixed_offset() {
    let matcher = MimeMagicMatcher::new(
        MagicValueType::String,
        0,
        0,
        b"%PDF-".to_vec(),
        None,
        vec![],
    )
    .expect("matcher should be valid");

    assert!(matcher.matches(b"%PDF-1.7"));
    assert!(!matcher.matches(b"not a pdf"));
}

#[test]
fn test_matches_string_in_offset_range() {
    let matcher = MimeMagicMatcher::new(
        MagicValueType::String,
        0,
        8,
        b"SQLite".to_vec(),
        None,
        vec![],
    )
    .expect("matcher should be valid");

    assert!(matcher.matches(b"xxSQLite format 3"));
    assert!(!matcher.matches(b"too far away SQLite"));
}

#[test]
fn test_matches_string_with_mask() {
    let matcher = MimeMagicMatcher::new(
        MagicValueType::String,
        0,
        0,
        b"BMxxxx\0\0".to_vec(),
        Some(vec![0xff, 0xff, 0, 0, 0, 0, 0xff, 0xff]),
        vec![],
    )
    .expect("matcher should be valid");

    assert!(matcher.matches(b"BM1234\0\0"));
    assert!(!matcher.matches(b"BA1234\0\0"));
}

#[test]
fn test_matches_byte_with_mask() {
    let matcher = MimeMagicMatcher::new(
        MagicValueType::Byte,
        3,
        3,
        vec![0x80],
        Some(vec![0x80]),
        vec![],
    )
    .expect("matcher should be valid");

    assert!(matcher.matches(&[0, 0, 0, 0x80]));
    assert!(matcher.matches(&[0, 0, 0, 0xff]));
    assert!(!matcher.matches(&[0, 0, 0, 0x7f]));
}

#[test]
fn test_matches_little32_integer() {
    let matcher = MimeMagicMatcher::new(
        MagicValueType::Little32,
        0,
        0,
        0xBEEFC0DEu32.to_be_bytes().to_vec(),
        None,
        vec![],
    )
    .expect("matcher should be valid");

    assert!(matcher.matches(&[0xde, 0xc0, 0xef, 0xbe]));
    assert!(!matcher.matches(&[0xbe, 0xef, 0xc0, 0xde]));
}

#[test]
fn test_matches_requires_one_submatcher_when_children_exist() {
    let child = MimeMagicMatcher::new(
        MagicValueType::String,
        4,
        4,
        b"child".to_vec(),
        None,
        vec![],
    )
    .expect("child matcher should be valid");
    let matcher = MimeMagicMatcher::new(
        MagicValueType::String,
        0,
        0,
        b"root".to_vec(),
        None,
        vec![child],
    )
    .expect("parent matcher should be valid");

    assert!(matcher.matches(b"rootchild"));
    assert!(!matcher.matches(b"rootxxxxx"));
}
