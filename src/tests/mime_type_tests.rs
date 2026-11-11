// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use crate::MimeRepository;
use crate::MimeType;

#[test]
fn test_inheritance_long_chain_uses_heap_traversal() {
    let depth = 16_384;
    let mut xml = String::from("<mime-info xmlns=\"http://www.freedesktop.org/standards/shared-mime-info\">");
    for index in 0..depth {
        xml.push_str(&format!("<mime-type type=\"application/x-node-{index}\">"));
        if index + 1 < depth {
            xml.push_str(&format!("<sub-class-of type=\"application/x-node-{}\"/>", index + 1));
        } else {
            xml.push_str("<magic priority=\"50\"><match type=\"string\" value=\"PROBE\" offset=\"0\"/></magic>");
        }
        xml.push_str("</mime-type>");
    }
    xml.push_str("</mime-info>");
    let repository = MimeRepository::from_xml(&xml).expect("long-chain repository should parse");
    let start = repository.get("application/x-node-0").expect("start node should exist");

    assert!(start.matches_content(&repository, b"PROBE"));
    assert!(!start.matches_content(&repository, b"MISS"));
}

#[test]
fn test_inheritance_traversal_preserves_depth_first_priority_and_rule_identity() {
    let repository = inheritance_priority_repository(50);
    let root = repository.get("application/x-root").expect("root should exist");
    let expected = &repository.get("application/x-p2").expect("p2 should exist").magics()[0];
    let matched = root
        .matched_magic(&repository, b"MATCH", 0)
        .expect("matching magic should exist");
    assert!(std::ptr::eq(expected, matched));

    let lower_p2_repository = inheritance_priority_repository(40);
    let root = lower_p2_repository
        .get("application/x-root")
        .expect("root should exist");
    let expected = &lower_p2_repository
        .get("application/x-g")
        .expect("g should exist")
        .magics()[0];
    let matched = root
        .matched_magic(&lower_p2_repository, b"MATCH", 0)
        .expect("matching magic should exist");
    assert!(std::ptr::eq(expected, matched));
    assert!(root.matched_magic(&lower_p2_repository, b"MATCH", 51).is_none());
}

#[test]
fn test_inheritance_traversal_handles_cycles_diamonds_and_missing_parents() {
    let repository = MimeRepository::from_xml(
        r#"<mime-info xmlns="http://www.freedesktop.org/standards/shared-mime-info">
  <mime-type type="application/x-root"><sub-class-of type="application/x-left"/><sub-class-of type="application/x-right"/><sub-class-of type="application/x-missing"/></mime-type>
  <mime-type type="application/x-left"><sub-class-of type="application/x-shared"/></mime-type>
  <mime-type type="application/x-right"><sub-class-of type="application/x-shared"/></mime-type>
  <mime-type type="application/x-shared"><sub-class-of type="application/x-root"/><magic priority="50"><match type="string" value="MATCH" offset="0"/></magic></mime-type>
</mime-info>"#,
    )
    .expect("graph with missing parent should parse");
    let root = repository.get("application/x-root").expect("root should exist");
    assert!(root.matches_content(&repository, b"MATCH"));
    assert!(!root.matches_content(&repository, b"MISS"));

    let ordered_repository = MimeRepository::from_xml(
        r#"<mime-info xmlns="http://www.freedesktop.org/standards/shared-mime-info">
  <mime-type type="application/x-root"><sub-class-of type="application/x-left"/><sub-class-of type="application/x-right"/><magic priority="50"><match type="string" value="MATCH" offset="0"/></magic></mime-type>
  <mime-type type="application/x-left"><sub-class-of type="application/x-shared"/></mime-type>
  <mime-type type="application/x-shared"><sub-class-of type="application/x-root"/><magic priority="50"><match type="string" value="MATCH" offset="0"/></magic></mime-type>
  <mime-type type="application/x-right"><sub-class-of type="application/x-shared"/><magic priority="50"><match type="string" value="MATCH" offset="0"/></magic></mime-type>
</mime-info>"#,
    )
    .expect("ordered graph should parse");
    let root = ordered_repository.get("application/x-root").expect("root should exist");
    let expected = &ordered_repository
        .get("application/x-right")
        .expect("right should exist")
        .magics()[0];
    let matched = root
        .matched_magic(&ordered_repository, b"MATCH", 0)
        .expect("matching magic should exist");
    assert!(std::ptr::eq(expected, matched));
}

fn inheritance_priority_repository(p2_priority: u8) -> MimeRepository {
    MimeRepository::from_xml(&format!(
        r#"<mime-info xmlns="http://www.freedesktop.org/standards/shared-mime-info">
  <mime-type type="application/x-root"><sub-class-of type="application/x-p1"/><sub-class-of type="application/x-p2"/><magic priority="50"><match type="string" value="MATCH" offset="0"/></magic></mime-type>
  <mime-type type="application/x-p1"><sub-class-of type="application/x-g"/><magic priority="50"><match type="string" value="MATCH" offset="0"/></magic></mime-type>
  <mime-type type="application/x-g"><magic priority="50"><match type="string" value="MATCH" offset="0"/></magic></mime-type>
  <mime-type type="application/x-p2"><magic priority="{p2_priority}"><match type="string" value="MATCH" offset="0"/></magic></mime-type>
</mime-info>"#,
    ))
    .expect("priority repository should parse")
}

#[test]
fn test_inheritance_public_matches_content_contract_remains_available() {
    let repository = MimeRepository::from_xml(
        r#"<mime-info xmlns="http://www.freedesktop.org/standards/shared-mime-info">
  <mime-type type="application/x-parent"><magic priority="50"><match type="string" value="MATCH" offset="0"/></magic></mime-type>
</mime-info>"#,
    )
    .expect("repository should parse");
    let child = MimeType::builder("application/x-child")
        .super_type("application/x-parent")
        .build()
        .expect("child should build");
    assert!(child.matches_content(&repository, b"MATCH"));
}
