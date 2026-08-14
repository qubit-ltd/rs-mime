// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Tests for MIME magic matcher rules.

use qubit_mime::MagicValueType;
use qubit_mime::MimeError;
use qubit_mime::MimeMagic;
use qubit_mime::MimeMagicMatcher;

#[test]
fn test_new_rejects_inverted_offset_range() {
    let error = MimeMagicMatcher::new(
        MagicValueType::String,
        5,
        4,
        b"ABC".to_vec(),
        None,
        vec![],
    )
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

#[test]
fn test_new_rejects_empty_value_bad_numeric_width_and_bad_mask_width() {
    assert!(matches!(
        MimeMagicMatcher::new(
            MagicValueType::String,
            0,
            0,
            Vec::new(),
            None,
            vec![]
        ),
        Err(MimeError::InvalidMagicMatcher { .. })
    ));
    assert!(matches!(
        MimeMagicMatcher::new(
            MagicValueType::Big16,
            0,
            0,
            vec![0],
            None,
            vec![]
        ),
        Err(MimeError::InvalidMagicMatcher { .. })
    ));
    assert!(matches!(
        MimeMagicMatcher::new(
            MagicValueType::String,
            0,
            0,
            b"ABC".to_vec(),
            Some(vec![0xff]),
            vec![],
        ),
        Err(MimeError::InvalidMagicMatcher { .. })
    ));
}

#[test]
fn test_accessors_and_numeric_endianness_matchers() {
    let big16 = MimeMagicMatcher::new(
        MagicValueType::Big16,
        0,
        0,
        0x1234u16.to_be_bytes().to_vec(),
        None,
        vec![],
    )
    .expect("big16 matcher should be valid");
    let big32 = MimeMagicMatcher::new(
        MagicValueType::Big32,
        0,
        0,
        0x12345678u32.to_be_bytes().to_vec(),
        Some(0xfffffff0u32.to_be_bytes().to_vec()),
        vec![],
    )
    .expect("big32 matcher should be valid");
    let host16 = MimeMagicMatcher::new(
        MagicValueType::Host16,
        0,
        0,
        0x1234u16.to_be_bytes().to_vec(),
        None,
        vec![],
    )
    .expect("host16 matcher should be valid");
    let host32 = MimeMagicMatcher::new(
        MagicValueType::Host32,
        0,
        0,
        0x12345678u32.to_be_bytes().to_vec(),
        None,
        vec![],
    )
    .expect("host32 matcher should be valid");

    assert_eq!(MagicValueType::Big16, big16.value_type());
    assert_eq!(0, big16.offset_begin());
    assert_eq!(0, big16.offset_end());
    assert_eq!(2, big16.value().len());
    assert!(big16.mask().is_none());
    assert!(big16.sub_matchers().is_empty());
    assert_eq!(2, big16.max_test_bytes());
    assert!(big16.matches(&0x1234u16.to_be_bytes()));
    assert!(!big16.matches(&[0]));
    assert!(big32.matches(&[0x12, 0x34, 0x56, 0x70]));
    assert!(host16.matches(&ordered_host16_bytes(0x1234)));
    assert!(host32.matches(&ordered_host32_bytes(0x12345678)));
}

fn ordered_host16_bytes(value: u16) -> [u8; 2] {
    if cfg!(target_endian = "little") {
        value.to_le_bytes()
    } else {
        value.to_be_bytes()
    }
}

fn ordered_host32_bytes(value: u32) -> [u8; 4] {
    if cfg!(target_endian = "little") {
        value.to_le_bytes()
    } else {
        value.to_be_bytes()
    }
}

#[test]
fn test_magic_value_type_names_and_lookup_cover_all_variants() {
    let variants = [
        (MagicValueType::String, "string"),
        (MagicValueType::Host16, "host16"),
        (MagicValueType::Host32, "host32"),
        (MagicValueType::Big16, "big16"),
        (MagicValueType::Big32, "big32"),
        (MagicValueType::Little16, "little16"),
        (MagicValueType::Little32, "little32"),
        (MagicValueType::Byte, "byte"),
    ];

    for (variant, name) in variants {
        assert_eq!(Some(variant), MagicValueType::from_name(name));
        assert_eq!(name, variant.name());
    }
    assert_eq!(None, MagicValueType::from_name("unknown"));
}

#[test]
fn test_mime_magic_empty_and_non_empty_matching() {
    let empty = MimeMagic::new(0, Vec::new());
    let matcher = MimeMagicMatcher::new(
        MagicValueType::String,
        0,
        0,
        b"ABC".to_vec(),
        None,
        vec![],
    )
    .expect("matcher should be valid");
    let magic = MimeMagic::new(25, vec![matcher]);

    assert_eq!(0, empty.priority());
    assert!(empty.matchers().is_empty());
    assert_eq!(0, empty.max_test_bytes());
    assert!(!empty.matches(b"ABC"));
    assert_eq!(25, magic.priority());
    assert_eq!(1, magic.matchers().len());
    assert_eq!(3, magic.max_test_bytes());
    assert!(magic.matches(b"ABC"));
}
