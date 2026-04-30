/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/

use qubit_mime::{FileCommandMimeDetector, MimeDetector};

#[test]
fn test_detect_by_filename_uses_repository_candidates() {
    let detector = FileCommandMimeDetector::new();

    assert_eq!(
        Some("image/jpeg".to_owned()),
        detector.detect_by_filename("photo.jpg")
    );
}

#[test]
fn test_is_available_can_be_called_without_panicking() {
    let _ = FileCommandMimeDetector::is_available();
}
