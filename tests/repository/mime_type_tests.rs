/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Tests for MIME type metadata helpers.

use qubit_mime::{
    MimeGlob,
    MimeRepository,
    MimeType,
};

#[test]
fn test_get_preferred_extension_prefers_simple_extension_over_complex_extension() {
    let simple = MimeGlob::new("*.pdf", 40, false).expect("simple glob should be valid");
    let complex = MimeGlob::new("*.tar.gz", 100, false).expect("complex glob should be valid");
    let mime_type = MimeType::builder("application/example")
        .description("", "Example")
        .glob(complex)
        .glob(simple)
        .build();

    assert_eq!(Some("pdf"), mime_type.preferred_extension());
}

#[test]
fn test_get_all_extensions_sorts_by_weight_descending() {
    let low = MimeGlob::new("*.low", 10, false).expect("glob should be valid");
    let high = MimeGlob::new("*.high", 90, false).expect("glob should be valid");
    let mime_type = MimeType::builder("application/example")
        .description("", "Example")
        .glob(low)
        .glob(high)
        .build();

    assert_eq!(vec!["high", "low"], mime_type.all_extensions());
}

#[test]
fn test_description_falls_back_to_default_and_english_entries() {
    let default_type = MimeType::builder("text/default")
        .description("", "Default description")
        .build();
    let english_type = MimeType::builder("text/english")
        .description("en", "English description")
        .build();

    assert_eq!(Some("Default description"), default_type.description());
    assert_eq!(Some("English description"), english_type.description());
}

#[test]
fn test_metadata_getters_filename_matching_and_supertype_content_matching() {
    let complex = MimeGlob::new("*.tar.gz", 80, false).expect("glob should be valid");
    let literal = MimeGlob::new("Makefile", 50, false).expect("glob should be valid");
    let mime_type = MimeType::builder("application/example")
        .alias("application/x-example")
        .glob(complex)
        .glob(literal)
        .super_type("application/base")
        .build();

    assert_eq!("application/example", mime_type.name());
    assert_eq!(None, mime_type.description());
    assert_eq!(&["application/x-example".to_owned()], mime_type.aliases());
    assert_eq!(2, mime_type.globs().len());
    assert!(mime_type.magics().is_empty());
    assert_eq!(&["application/base".to_owned()], mime_type.super_types());
    assert_eq!(Some("tar.gz"), mime_type.preferred_extension());
    assert_eq!(vec!["tar.gz"], mime_type.all_extensions());
    assert!(mime_type.matches_filename("Makefile"));
    assert!(!mime_type.matches_filename("Cargo.toml"));
    assert_eq!(mime_type, mime_type.clone());

    let repository = MimeRepository::from_xml(
        r#"
<mime-info>
  <mime-type type="application/base">
    <comment>base</comment>
    <magic priority="50"><match type="string" value="BASE" offset="0"/></magic>
  </mime-type>
  <mime-type type="application/child">
    <comment>child</comment>
    <sub-class-of type="application/base"/>
  </mime-type>
</mime-info>
"#,
    )
    .expect("repository should parse");
    let child = repository
        .get("application/child")
        .expect("child type should exist");

    assert!(child.matches_content(&repository, b"BASE data"));
    assert!(!child.matches_content(&repository, b"MISS"));
}
