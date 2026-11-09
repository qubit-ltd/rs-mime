// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_mime::MimeError;

#[test]
fn test_mime_detector_unavailable_error_reports_name_and_reason() {
    let unavailable = MimeError::DetectorUnavailable {
        name: "file".to_owned(),
        reason: "missing command".to_owned(),
    };

    assert!(unavailable.to_string().contains("file"));
    assert!(unavailable.to_string().contains("missing command"));
}
