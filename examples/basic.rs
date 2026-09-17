// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Detect a user-uploaded file by name and content.

use qubit_mime::MimeDetectionPolicy;
use qubit_mime::RepositoryMimeDetector;

fn main() -> qubit_mime::MimeResult<()> {
    let detector = RepositoryMimeDetector::new()?;
    assert_eq!(
        detector.detect_by_filename("report.pdf")?.as_deref(),
        Some("application/pdf"),
    );
    assert_eq!(
        detector
            .detect_bytes(b"%PDF-1.7\n", Some("report.pdf"), MimeDetectionPolicy::VerifyContent)?
            .as_deref(),
        Some("application/pdf"),
    );
    assert_eq!(
        detector
            .detect_bytes(b"%PDF-1.7\n", Some("report.jpg"), MimeDetectionPolicy::VerifyContent)?
            .as_deref(),
        Some("application/pdf"),
    );
    Ok(())
}
