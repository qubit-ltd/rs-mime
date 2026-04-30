/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Coverage-only tests for defensive and synthetic branches.

#[test]
fn test_exercise_coverage_support_branches() {
    let result = qubit_mime::coverage_support::exercise_all();

    assert!(!result.is_empty());
    assert!(result.iter().any(|entry| entry.contains("root element")));
}
