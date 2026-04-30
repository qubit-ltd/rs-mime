/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/

use qubit_mime::{MimeDetector, RepositoryMimeDetector};

#[test]
fn test_mime_detector_trait_supports_repository_detector() {
    let mut detector = RepositoryMimeDetector::new().expect("default repository should load");
    assert!(!MimeDetector::is_always_check_magic_by_default(&detector));

    MimeDetector::set_always_check_magic_by_default(&mut detector, true);
    assert!(MimeDetector::is_always_check_magic_by_default(&detector));

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
        detector.detect(b"%PDF-1.7\n", Some("photo.jpg"), true)
    );
}

#[test]
fn test_default_mime_detector_returns_usable_detector() {
    let detector = <dyn MimeDetector>::default_detector();
    assert!(detector.detect_by_filename("document.pdf").is_some());
}
