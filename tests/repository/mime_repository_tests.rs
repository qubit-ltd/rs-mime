/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Tests for MIME repository parsing and matching.

use qubit_mime::{
    MimeRepository,
    MimeType,
};

const TEST_DATABASE: &str = r#"
<mime-info>
  <mime-type type="application/gzip">
    <comment>gzip archive</comment>
    <glob pattern="*.gz"/>
    <magic priority="60">
      <match type="string" value="\x1f\x8b" offset="0"/>
    </magic>
  </mime-type>
  <mime-type type="application/x-compressed-tar">
    <comment>compressed tar archive</comment>
    <alias type="application/x-gtar"/>
    <glob pattern="*.tar.gz" weight="60"/>
  </mime-type>
  <mime-type type="image/png">
    <comment>PNG image</comment>
    <glob pattern="*.png"/>
    <magic priority="50">
      <match type="string" value="\x89PNG\r\n\x1a\n" offset="0"/>
    </magic>
  </mime-type>
  <mime-type type="text/x-makefile">
    <comment>Makefile</comment>
    <glob pattern="Makefile"/>
    <glob pattern="README*"/>
  </mime-type>
  <mime-type type="text/x-csrc">
    <comment>C source</comment>
    <glob pattern="*.c" case-sensitive="true"/>
  </mime-type>
  <mime-type type="text/x-c++src">
    <comment>C++ source</comment>
    <glob pattern="*.C" case-sensitive="true"/>
  </mime-type>
  <mime-type type="application/pdf">
    <comment>PDF document</comment>
    <glob pattern="*.pdf"/>
    <magic priority="50">
      <match type="string" value="%PDF-" offset="0:1024"/>
    </magic>
  </mime-type>
  <mime-type type="application/x-byte-mask">
    <comment>masked byte</comment>
    <magic priority="80">
      <match type="byte" value="0x80" mask="0x80" offset="3"/>
    </magic>
  </mime-type>
  <mime-type type="application/x-nested">
    <comment>nested matcher</comment>
    <magic priority="90">
      <match type="string" value="ROOT" offset="0">
        <match type="little32" value="0xBEEFC0DE" offset="4"/>
      </match>
    </magic>
  </mime-type>
</mime-info>
"#;

fn create_repository() -> MimeRepository {
    MimeRepository::from_xml(TEST_DATABASE).expect("test database should parse")
}

fn names(mime_types: Vec<&MimeType>) -> Vec<String> {
    mime_types
        .iter()
        .map(|mime_type| mime_type.name().to_owned())
        .collect()
}

#[test]
fn test_from_xml_indexes_names_aliases_and_max_test_bytes() {
    let repository = create_repository();

    assert_eq!(9, repository.all().len());
    assert_eq!(
        Some("application/x-compressed-tar"),
        repository.get("application/x-gtar").map(MimeType::name)
    );
    assert!(repository.max_test_bytes() >= 1029);
}

#[test]
fn test_detect_by_filename_prefers_longer_equal_weight_extension() {
    let repository = create_repository();

    assert_eq!(
        vec!["application/x-compressed-tar"],
        names(repository.detect_by_filename("archive.tar.gz"))
    );
    assert_eq!(
        vec!["application/gzip"],
        names(repository.detect_by_filename("archive.gz"))
    );
}

#[test]
fn test_detect_by_filename_handles_literal_other_and_case_sensitive_globs() {
    let repository = create_repository();

    assert_eq!(
        vec!["text/x-makefile"],
        names(repository.detect_by_filename("/tmp/Makefile"))
    );
    assert_eq!(
        vec!["text/x-makefile"],
        names(repository.detect_by_filename("README.zh_CN.md"))
    );
    assert_eq!(
        vec!["text/x-c++src"],
        names(repository.detect_by_filename("main.C"))
    );
    assert_eq!(
        vec!["text/x-csrc"],
        names(repository.detect_by_filename("main.c"))
    );
}

#[test]
fn test_detect_by_content_orders_by_magic_priority() {
    let repository = create_repository();

    assert_eq!(
        vec!["image/png"],
        names(repository.detect_by_content(b"\x89PNG\r\n\x1a\nextra"))
    );
    assert_eq!(
        vec!["application/x-byte-mask"],
        names(repository.detect_by_content(&[0, 0, 0, 0xff]))
    );
    assert_eq!(
        vec!["application/x-nested"],
        names(repository.detect_by_content(&[b'R', b'O', b'O', b'T', 0xde, 0xc0, 0xef, 0xbe,]))
    );
}

#[test]
fn test_detect_uses_filename_when_single_candidate_and_magic_not_required() {
    let repository = create_repository();

    assert_eq!(
        vec!["application/pdf"],
        names(repository.detect(
            "document.pdf",
            b"\x89PNG\r\n\x1a\n",
            qubit_mime::MimeDetectionPolicy::PreferFilename,
        ))
    );
}

#[test]
fn test_detect_uses_magic_when_verify_content_policy_is_enabled() {
    let repository = create_repository();

    assert_eq!(
        vec!["image/png"],
        names(repository.detect(
            "document.pdf",
            b"\x89PNG\r\n\x1a\n",
            qubit_mime::MimeDetectionPolicy::VerifyContent,
        ))
    );
}

#[test]
fn test_detect_returns_empty_when_no_rule_matches() {
    let repository = create_repository();

    assert!(repository.detect_by_filename("unknown.nope").is_empty());
    assert!(
        repository
            .detect_by_content(b"nothing recognizable")
            .is_empty()
    );
    assert!(
        repository
            .detect(
                "unknown.nope",
                b"nothing recognizable",
                qubit_mime::MimeDetectionPolicy::VerifyContent,
            )
            .is_empty()
    );
}

#[test]
fn test_from_xml_accepts_doctype_and_reports_structural_errors() {
    let repository = MimeRepository::from_xml(
        r#"
<!DOCTYPE mime-info [
<!ELEMENT mime-info (mime-type)+>
]>
<mime-info>
  <mime-type type="text/plain">
    <comment>plain</comment>
    <glob pattern="*.txt"/>
  </mime-type>
</mime-info>
"#,
    )
    .expect("DTD-stripped repository should parse");

    assert_eq!(
        Some("text/plain"),
        repository.get("text/plain").map(MimeType::name)
    );
    assert!(
        MimeRepository::from_xml("<bad/>")
            .expect_err("bad root should fail")
            .to_string()
            .contains("root element")
    );
    assert!(
        MimeRepository::from_xml(
            "<mime-info><mime-type><comment>x</comment></mime-type></mime-info>",
        )
        .expect_err("missing type should fail")
        .to_string()
        .contains("attribute")
    );
    assert!(
        MimeRepository::from_xml("<!DOCTYPE mime-info [ <mime-info>")
            .expect_err("malformed DTD should fail")
            .to_string()
            .contains("XML")
    );
}

#[test]
fn test_from_xml_reports_invalid_glob_and_magic_attributes() {
    let cases = [
        (
            r#"<mime-info><mime-type type="x"><comment>x</comment><glob pattern="*.x" weight="101"/></mime-type></mime-info>"#,
            "weight",
        ),
        (
            r#"<mime-info><mime-type type="x"><comment>x</comment><glob pattern="*.x" weight="abc"/></mime-type></mime-info>"#,
            "weight",
        ),
        (
            r#"<mime-info><mime-type type="x"><comment>x</comment><glob pattern="*.x" case-sensitive="maybe"/></mime-type></mime-info>"#,
            "case-sensitive",
        ),
        (
            r#"<mime-info><mime-type type="x"><comment>x</comment><magic priority="101"><match type="string" value="x" offset="0"/></magic></mime-type></mime-info>"#,
            "priority",
        ),
        (
            r#"<mime-info><mime-type type="x"><comment>x</comment><magic/></mime-type></mime-info>"#,
            "magic",
        ),
        (
            r#"<mime-info><mime-type type="x"><comment>x</comment><magic><match type="unknown" value="x" offset="0"/></magic></mime-type></mime-info>"#,
            "type",
        ),
        (
            r#"<mime-info><mime-type type="x"><comment>x</comment><magic><match type="string" value="x" offset="2:1"/></magic></mime-type></mime-info>"#,
            "offset",
        ),
        (
            r#"<mime-info><mime-type type="x"><comment>x</comment><magic><match type="string" value="x" offset="bad"/></magic></mime-type></mime-info>"#,
            "offset",
        ),
    ];

    for (xml, expected) in cases {
        let error = MimeRepository::from_xml(xml).expect_err("invalid repository XML should fail");
        assert!(
            error.to_string().contains(expected),
            "error `{error}` should contain `{expected}`"
        );
    }
}

#[test]
fn test_from_xml_reports_invalid_string_and_numeric_magic_values() {
    let cases = [
        r#"<mime-info><mime-type type="x"><comment>x</comment><magic><match type="string" value="\" offset="0"/></magic></mime-type></mime-info>"#,
        r#"<mime-info><mime-type type="x"><comment>x</comment><magic><match type="string" value="\x" offset="0"/></magic></mime-type></mime-info>"#,
        r#"<mime-info><mime-type type="x"><comment>x</comment><magic><match type="string" value="x" mask="ff" offset="0"/></magic></mime-type></mime-info>"#,
        r#"<mime-info><mime-type type="x"><comment>x</comment><magic><match type="string" value="x" mask="0xf" offset="0"/></magic></mime-type></mime-info>"#,
        r#"<mime-info><mime-type type="x"><comment>x</comment><magic><match type="string" value="x" mask="0xgg" offset="0"/></magic></mime-type></mime-info>"#,
        r#"<mime-info><mime-type type="x"><comment>x</comment><magic><match type="byte" value="bad" offset="0"/></magic></mime-type></mime-info>"#,
    ];

    for xml in cases {
        assert!(
            MimeRepository::from_xml(xml).is_err(),
            "invalid magic value should fail: {xml}"
        );
    }
}

#[test]
fn test_detect_by_filename_and_content_preserve_ties() {
    let repository = MimeRepository::from_xml(
        r#"
<mime-info>
  <mime-type type="text/short"><comment>short</comment><glob pattern="READ*" weight="50"/></mime-type>
  <mime-type type="text/long"><comment>long</comment><glob pattern="README*" weight="50"/></mime-type>
  <mime-type type="text/tie-one"><comment>tie one</comment><glob pattern="*.tie" weight="50"/></mime-type>
  <mime-type type="text/tie-two"><comment>tie two</comment><glob pattern="*.tie" weight="50"/></mime-type>
  <mime-type type="application/magic-one"><comment>magic one</comment><magic priority="50"><match type="string" value="TIE" offset="0"/></magic></mime-type>
  <mime-type type="application/magic-two"><comment>magic two</comment><magic priority="50"><match type="string" value="TIE" offset="0"/></magic></mime-type>
</mime-info>
"#,
    )
    .expect("tie repository should parse");

    assert_eq!(
        Some("text/long"),
        repository
            .detect_by_filename("README.md")
            .first()
            .map(|mime_type| mime_type.name())
    );
    assert_eq!(2, repository.detect_by_filename("file.tie").len());
    assert_eq!(2, repository.detect_by_content(b"TIE").len());
}
