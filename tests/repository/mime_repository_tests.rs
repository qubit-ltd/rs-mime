/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Tests for MIME repository parsing and matching.

use qubit_mime::{MimeRepository, MimeType};

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
        names(repository.detect("document.pdf", b"\x89PNG\r\n\x1a\n", false))
    );
}

#[test]
fn test_detect_uses_magic_when_always_check_magic_is_enabled() {
    let repository = create_repository();

    assert_eq!(
        vec!["image/png"],
        names(repository.detect("document.pdf", b"\x89PNG\r\n\x1a\n", true))
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
            .detect("unknown.nope", b"nothing recognizable", true)
            .is_empty()
    );
}
