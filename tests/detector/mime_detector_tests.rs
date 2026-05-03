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
use tempfile::NamedTempFile;

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
fn test_mime_detector_trait_supports_reader_and_file_detection() {
    let detector = RepositoryMimeDetector::new().expect("default repository should load");
    let detector: &dyn MimeDetector = &detector;
    let mut reader = std::io::Cursor::new(b"%PDF-1.7\n".to_vec());

    let from_reader = detector
        .detect_reader(
            &mut reader,
            Some("document.pdf"),
            MimeDetectionPolicy::VerifyContent,
        )
        .expect("trait-object reader detection should succeed");

    let mut file = NamedTempFile::with_suffix(".pdf").expect("temp file should be created");
    std::io::Write::write_all(&mut file, b"%PDF-1.7\n").expect("temp file should be writable");
    let from_file = detector
        .detect_file(file.path(), MimeDetectionPolicy::VerifyContent)
        .expect("trait-object file detection should succeed");

    assert_eq!(Some("application/pdf".to_owned()), from_reader);
    assert_eq!(Some("application/pdf".to_owned()), from_file);
    assert_eq!(0, reader.position());
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
