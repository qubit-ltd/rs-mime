// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_mime::MimeError;

/// MIME-domain errors retain detector context independently of SPI internals.
#[test]
fn test_mime_error_display_includes_variant_context() {
    let duplicate = MimeError::DuplicateDetectorName {
        name: "repository".to_owned(),
    };
    let backend = MimeError::detector_backend("file", "command failed");

    assert_eq!(
        "duplicate MIME detector name or alias: repository",
        duplicate.to_string()
    );
    assert_eq!(
        "MIME detector backend 'file' failed: command failed",
        backend.to_string(),
    );
}
