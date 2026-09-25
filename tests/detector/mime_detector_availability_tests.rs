// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_mime::MimeDetectorAvailability;

#[test]
fn test_mime_detector_availability_reports_state_and_reason() {
    assert!(MimeDetectorAvailability::Available.is_available());

    let unavailable = MimeDetectorAvailability::Unavailable {
        reason: "missing command".to_owned(),
    };

    assert!(!unavailable.is_available());
    assert!(format!("{unavailable:?}").contains("missing command"));
}
