// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::borrow::Cow;

use crate::repository::xml_parser::strip_doctype;

#[test]
fn preserves_doctype_like_text_in_cdata_and_comments() {
    let xml = "<comment><![CDATA[example <!DOCTYPE demo []> suffix]]></comment>\n<!-- <!DOCTYPE fake []> -->";
    assert_eq!(strip_doctype(xml), Cow::Borrowed(xml));
}

#[test]
fn leaves_xml_without_doctype_borrowed() {
    let xml = "\u{feff}<?xml version=\"1.0\"?>\n<!-- preface -->\n<mime-info/>";
    assert!(matches!(strip_doctype(xml), Cow::Borrowed(value) if value == xml));
}

#[test]
fn removes_declaration_without_internal_subset_and_preserves_other_bytes() {
    let xml = "<?xml version=\"1.0\"?>\n<!DOCTYPE mime-info SYSTEM \"mime.dtd\">\n<mime-info/>";
    assert_eq!(
        strip_doctype(xml),
        "<?xml version=\"1.0\"?>\n\n<mime-info/>"
    );
}

#[test]
fn declaration_terminators_in_quotes_and_subset_comments_are_ignored() {
    let xml = "<!DOCTYPE mime-info SYSTEM \"file]>name\" [<!-- ]> -->\n<!ENTITY x \"value > ]>\">]><mime-info/>";
    assert_eq!(strip_doctype(xml), "<mime-info/>");
}

#[test]
fn leaves_truncated_declaration_unchanged() {
    let xml = "<!DOCTYPE mime-info [<!-- unterminated ]>";
    assert!(matches!(strip_doctype(xml), Cow::Borrowed(value) if value == xml));
}

#[test]
fn stops_scanning_at_root_element() {
    let xml = "<mime-info><!-- <!DOCTYPE fake []> --></mime-info>";
    assert!(matches!(strip_doctype(xml), Cow::Borrowed(value) if value == xml));
}

#[test]
fn leaves_malformed_or_repeated_declarations_for_the_xml_parser() {
    let malformed = "<!DOCTYPE mime-info [] unexpected><mime-info/>";
    assert!(matches!(strip_doctype(malformed), Cow::Borrowed(value) if value == malformed));

    let repeated = "<!DOCTYPE mime-info><!DOCTYPE mime-info><mime-info/>";
    assert_eq!(strip_doctype(repeated), "<!DOCTYPE mime-info><mime-info/>");
}
