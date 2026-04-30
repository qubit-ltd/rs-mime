/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Tests for MIME type metadata helpers.

use qubit_mime::{MimeGlob, MimeType};

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
