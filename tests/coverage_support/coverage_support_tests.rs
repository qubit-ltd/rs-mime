/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Coverage-only tests for defensive and synthetic branches.

#[test]
fn test_exercise_coverage_support_branches() {
    let result = qubit_mime::coverage_support::exercise_all();

    assert!(!result.is_empty());
    assert!(result.iter().any(|entry| entry.contains("root element")));
}
