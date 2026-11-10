// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::collections::hash_map::DefaultHasher;
use std::hash::Hash;
use std::hash::Hasher;

use qubit_mime::MagicValueType;
use qubit_mime::MimeError;
use qubit_mime::MimeGlob;
use qubit_mime::MimeMagic;
use qubit_mime::MimeMagicMatcher;
use qubit_mime::MimeTypeBuilder;

#[test]
fn test_mime_type_builder_rejects_invalid_name() {
    assert!(matches!(
        MimeTypeBuilder::new("not-a-mime").build(),
        Err(MimeError::InvalidMimeName { .. })
    ));
}

#[test]
fn test_mime_type_builder_collects_metadata_and_matching_rules() {
    let matcher = MimeMagicMatcher::new(MagicValueType::String, 0, 0, b"DATA".to_vec(), None, vec![]).unwrap();
    let mime_type = MimeTypeBuilder::new("application/x-data")
        .description("", "Data file")
        .alias("application/data")
        .glob(MimeGlob::new("*.data", 80, false).unwrap())
        .magic(MimeMagic::new(50, vec![matcher]).unwrap())
        .super_type("application/octet-stream")
        .build()
        .unwrap();

    assert_eq!("application/x-data", mime_type.name());
    assert_eq!(Some("Data file"), mime_type.description());
    assert_eq!(&["application/data".to_owned()], mime_type.aliases());
    assert_eq!(Some("data"), mime_type.preferred_extension());
    assert_eq!(1, mime_type.magics().len());
    assert_eq!(&["application/octet-stream".to_owned()], mime_type.super_types());
}

#[test]
fn test_builder_normalizes_canonical_alias_and_supertype_names() {
    let upper = MimeTypeBuilder::new("TEXT/PLAIN").build().unwrap();
    let lower = MimeTypeBuilder::new("text/plain").build().unwrap();
    let with_names = MimeTypeBuilder::new("APPLICATION/X-CHILD")
        .alias("APPLICATION/X-ALIAS")
        .alias("application/x-alias")
        .super_type("TEXT/X-NOTE")
        .super_type("text/x-note")
        .build()
        .unwrap();
    let mut upper_hash = DefaultHasher::new();
    let mut lower_hash = DefaultHasher::new();
    upper.name().hash(&mut upper_hash);
    lower.name().hash(&mut lower_hash);

    assert_eq!("text/plain", upper.name());
    assert_eq!(upper, lower);
    assert_eq!(upper_hash.finish(), lower_hash.finish());
    assert_eq!(&["application/x-alias".to_owned()], with_names.aliases());
    assert_eq!(&["text/x-note".to_owned()], with_names.super_types());
}
