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
fn test_preserves_doctype_like_text_in_cdata_and_comments() {
    let xml = "<comment><![CDATA[example <!DOCTYPE demo []> suffix]]></comment>\n<!-- <!DOCTYPE fake []> -->";
    assert_eq!(strip_doctype(xml), Cow::Borrowed(xml));
}

#[test]
fn test_leaves_xml_without_doctype_borrowed() {
    let xml = "\u{feff}<?xml version=\"1.0\"?>\n<!-- preface -->\n<mime-info/>";
    assert!(matches!(strip_doctype(xml), Cow::Borrowed(value) if value == xml));
}

#[test]
fn test_removes_declaration_without_internal_subset_and_preserves_other_bytes() {
    let xml = "<?xml version=\"1.0\"?>\n<!DOCTYPE mime-info SYSTEM \"mime.dtd\">\n<mime-info/>";
    assert_eq!(strip_doctype(xml), "<?xml version=\"1.0\"?>\n\n<mime-info/>");
}

#[test]
fn test_declaration_terminators_in_quotes_and_subset_comments_are_ignored() {
    let xml = "<!DOCTYPE mime-info SYSTEM \"file]>name\" [<!-- ]> -->\n<!ENTITY x \"value > ]>\">]><mime-info/>";
    assert_eq!(strip_doctype(xml), "<mime-info/>");
}

#[test]
fn test_valid_entity_notation_and_attribute_list_declarations_are_removed() {
    for xml in [
        "<!DOCTYPE r [<!ENTITY x SYSTEM \"entity.dtd\" NDATA n>]><r/>",
        "<!DOCTYPE r [<!NOTATION n PUBLIC \"public-id\">]><r/>",
        "<!DOCTYPE r [<!ATTLIST r token (1|2) #IMPLIED>]><r/>",
        "<!DOCTYPE r [<!ATTLIST r id ID #REQUIRED refs IDREFS #IMPLIED names NMTOKENS #IMPLIED>]><r/>",
        "<!DOCTYPE r [<!ELEMENT x (a)>]><r/>",
    ] {
        assert_eq!(strip_doctype(xml), "<r/>", "{xml}");
    }
}

#[test]
fn test_removes_doctype_with_unicode_element_name_and_nmtoken_enumeration() {
    let xml = "<!DOCTYPE 名 [<!ELEMENT 名 EMPTY><!ATTLIST 名 token (\u{301}|1) #IMPLIED>]><名/>";
    assert_eq!(strip_doctype(xml), "<名/>");
}

#[test]
fn test_leaves_truncated_unicode_element_name_unchanged() {
    let xml = "<!DOCTYPE r [<!ELEMENT r (名|";
    assert!(matches!(strip_doctype(xml), Cow::Borrowed(value) if value == xml));
}

#[test]
fn test_removes_doctype_with_many_element_declarations_and_preserves_long_body() {
    let declarations = (0..512)
        .map(|index| format!("<!ELEMENT e{index} EMPTY>"))
        .collect::<String>();
    let body = "body text ".repeat(16_384);
    let xml = format!("<!DOCTYPE r [{declarations}]><r>{body}</r>");
    let stripped = strip_doctype(&xml);
    assert_eq!(stripped, format!("<r>{body}</r>"));
}

#[test]
fn test_leaves_truncated_declaration_unchanged() {
    let xml = "<!DOCTYPE mime-info [<!-- unterminated ]>";
    assert!(matches!(strip_doctype(xml), Cow::Borrowed(value) if value == xml));
}

#[test]
fn test_stops_scanning_at_root_element() {
    let xml = "<mime-info><!-- <!DOCTYPE fake []> --></mime-info>";
    assert!(matches!(strip_doctype(xml), Cow::Borrowed(value) if value == xml));
}

#[test]
fn test_leaves_malformed_or_repeated_declarations_for_the_xml_parser() {
    let malformed = "<!DOCTYPE mime-info [] unexpected><mime-info/>";
    assert!(matches!(strip_doctype(malformed), Cow::Borrowed(value) if value == malformed));

    let missing_separator = "<!DOCTYPEevil><mime-info/>";
    assert!(matches!(strip_doctype(missing_separator), Cow::Borrowed(value) if value == missing_separator));

    let missing_name = "<!DOCTYPE ><mime-info/>";
    assert!(matches!(strip_doctype(missing_name), Cow::Borrowed(value) if value == missing_name));

    let invalid_external_id = "<!DOCTYPE mime-info garbage><mime-info/>";
    assert!(matches!(strip_doctype(invalid_external_id), Cow::Borrowed(value) if value == invalid_external_id));

    for malformed_subset in [
        "<!DOCTYPE mime-info [garbage]><mime-info/>",
        "<!DOCTYPE mime-info [<bogus>]><mime-info/>",
        "<!DOCTYPE mime-info [<!-- ok --> garbage]><mime-info/>",
        "<!DOCTYPE mime-info [<!ENTITY >]><mime-info/>",
        "<!DOCTYPE mime-info [<!ENTITY x \"value\" garbage>]><mime-info/>",
        "<!DOCTYPE mime-info [<!ELEMENT x EMPTYjunk>]><mime-info/>",
        "<!DOCTYPE mime-info [<!ELEMENT x (|a)>]><mime-info/>",
        "<!DOCTYPE mime-info [<!ELEMENT x (a|)>]><mime-info/>",
        "<!DOCTYPE mime-info [<!-- a -- b -->]><mime-info/>",
    ] {
        assert!(matches!(strip_doctype(malformed_subset), Cow::Borrowed(value) if value == malformed_subset));
    }

    let invalid_xml_character = "<!DOCTYPE mime-info SYSTEM \"\u{1}\"><mime-info/>";
    assert!(matches!(strip_doctype(invalid_xml_character), Cow::Borrowed(value) if value == invalid_xml_character));

    let repeated = "<!DOCTYPE mime-info><!DOCTYPE mime-info><mime-info/>";
    assert!(matches!(strip_doctype(repeated), Cow::Borrowed(value) if value == repeated));
}
