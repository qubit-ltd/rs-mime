// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_mime::MimeDetectionPolicy;

#[test]
fn test_mime_detection_policy_is_copyable_and_comparable() {
    let policy = MimeDetectionPolicy::PreferFilename;
    let copied = policy;

    assert_eq!(MimeDetectionPolicy::PreferFilename, copied);
    assert_ne!(MimeDetectionPolicy::VerifyContent, copied);
    assert_eq!("PreferFilename", format!("{policy:?}"));
}
