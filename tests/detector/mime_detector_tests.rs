/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/

use qubit_mime::{
    ArcMimeDetector, BoxMimeDetector, MimeDetectionPolicy, MimeDetector, RepositoryMimeDetector,
};

#[test]
fn test_mime_detector_trait_supports_repository_detector() {
    let detector = RepositoryMimeDetector::new().expect("default repository should load");
    let detector: &dyn MimeDetector = &detector;
    assert_eq!(
        Some("image/jpeg".to_owned()),
        detector.detect_by_filename("photo.JPG")
    );
    assert_eq!(
        Some("application/pdf".to_owned()),
        detector.detect_by_content(b"%PDF-1.7\n")
    );
    assert_eq!(
        Some("application/pdf".to_owned()),
        detector.detect(
            b"%PDF-1.7\n",
            Some("photo.jpg"),
            MimeDetectionPolicy::VerifyContent,
        )
    );
}

#[test]
fn test_default_mime_detector_returns_usable_detector() {
    let detector = BoxMimeDetector::default();
    assert!(detector.detect_by_filename("document.pdf").is_some());
}

#[test]
fn test_mime_detector_wrappers_select_named_detectors() {
    let boxed = BoxMimeDetector::from_name("repository").expect("repository detector");
    let shared = ArcMimeDetector::from_name("repository").expect("repository detector");

    assert_eq!(
        Some("application/pdf".to_owned()),
        boxed.detect_by_filename("document.pdf")
    );
    assert_eq!(
        Some("image/png".to_owned()),
        shared.detect_by_filename("image.png")
    );
    assert!(BoxMimeDetector::from_name("unknown").is_none());
    assert!(ArcMimeDetector::from_name("unknown").is_none());
}
