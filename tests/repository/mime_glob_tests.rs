// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Tests for MIME glob matching.

use qubit_mime::MimeError;
use qubit_mime::MimeGlob;

#[test]
fn test_new_rejects_weight_above_maximum() {
    let error = MimeGlob::new("*.txt", 101, false).expect_err("weight 101 should fail");

    assert!(matches!(error, MimeError::InvalidGlobWeight { weight: 101 }));
}

#[test]
fn test_matches_uses_case_insensitive_matching_by_default() {
    let glob = MimeGlob::new("*.PNG", 50, false).expect("glob should be valid");

    assert!(glob.matches("image.png"));
    assert!(glob.matches("IMAGE.PNG"));
    assert!(!glob.matches("image.jpg"));
}

#[test]
fn test_matches_honors_case_sensitive_flag() {
    let glob = MimeGlob::new("*.C", 50, true).expect("glob should be valid");

    assert!(glob.matches("main.C"));
    assert!(!glob.matches("main.c"));
}

#[test]
fn test_matches_supports_other_glob_tokens() {
    let numbered = MimeGlob::new("[0-9][0-9][0-9].vdr", 50, false).expect("glob should be valid");
    let wildcard = MimeGlob::new("README*", 10, false).expect("glob should be valid");

    assert!(numbered.matches("123.vdr"));
    assert!(!numbered.matches("abc.vdr"));
    assert!(wildcard.matches("README.zh_CN.md"));
}

#[test]
fn test_matches_returns_false_for_empty_inputs() {
    let empty_pattern = MimeGlob::new("", 50, false).expect("empty glob is allowed");
    let normal_pattern = MimeGlob::new("*.txt", 50, false).expect("glob should be valid");

    assert!(!empty_pattern.matches("readme.txt"));
    assert!(!normal_pattern.matches(""));
}

#[test]
fn test_matches_supports_question_negated_and_literal_class_edges() {
    let question = MimeGlob::new("file?.txt", 50, false).expect("glob should compile");
    let negated_class = MimeGlob::new("[!a]ile.txt", 50, false).expect("glob should compile");
    let unclosed_class = MimeGlob::new("file[.txt", 50, false).expect("glob should compile");
    let escaped_class = MimeGlob::new("[\\]a].txt", 50, false).expect("glob should compile");

    assert!(question.matches("file1.txt"));
    assert!(!question.matches("file12.txt"));
    assert!(negated_class.matches("bile.txt"));
    assert!(!negated_class.matches("aile.txt"));
    assert!(unclosed_class.matches("file[.txt"));
    assert!(escaped_class.matches("\\a].txt"));
    assert_eq!("file?.txt", question.pattern());
    assert_eq!(50, question.weight());
    assert!(!question.case_sensitive());
}
