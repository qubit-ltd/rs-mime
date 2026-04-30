/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Tests for the repository-backed MIME detector.

use std::io::Cursor;

use qubit_mime::{MimeDetectionPolicy, RepositoryMimeDetector};
use tempfile::NamedTempFile;

#[test]
fn test_detect_by_filename_uses_default_repository() {
    let detector = RepositoryMimeDetector::new().expect("default repository should load");

    assert_eq!(
        Some("image/jpeg".to_owned()),
        detector.detect_by_filename("photo.JPG")
    );
    assert_eq!(
        Some("application/x-compressed-tar".to_owned()),
        detector.detect_by_filename("/tmp/archive.tar.gz")
    );
    assert_eq!(None, detector.detect_by_filename(""));
}

#[test]
fn test_detect_by_content_uses_default_repository_magic() {
    let detector = RepositoryMimeDetector::new().expect("default repository should load");

    assert_eq!(
        Some("application/pdf".to_owned()),
        detector.detect_by_content(b"%PDF-1.7\n")
    );
    assert_eq!(
        Some("image/png".to_owned()),
        detector.detect_by_content(b"\x89PNG\r\n\x1a\n")
    );
}

#[test]
fn test_detect_bytes_merges_filename_and_content_results() {
    let detector = RepositoryMimeDetector::new().expect("default repository should load");

    assert_eq!(
        Some("image/jpeg".to_owned()),
        detector.detect_bytes(
            b"%PDF-1.7\n",
            Some("photo.jpg"),
            MimeDetectionPolicy::PreferFilename,
        )
    );
    assert_eq!(
        Some("application/pdf".to_owned()),
        detector.detect_bytes(
            b"%PDF-1.7\n",
            Some("photo.jpg"),
            MimeDetectionPolicy::VerifyContent,
        )
    );
    assert_eq!(
        Some("application/pdf".to_owned()),
        detector.detect_bytes(b"%PDF-1.7\n", None, MimeDetectionPolicy::VerifyContent)
    );
}

#[test]
fn test_detect_reader_does_not_consume_reader_position() {
    let detector = RepositoryMimeDetector::new().expect("default repository should load");
    let mut reader = Cursor::new(b"%PDF-1.7\n".to_vec());

    let detected = detector
        .detect_reader(
            &mut reader,
            Some("document.pdf"),
            MimeDetectionPolicy::VerifyContent,
        )
        .expect("reader detection should succeed");

    assert_eq!(Some("application/pdf".to_owned()), detected);
    assert_eq!(0, reader.position());
}

#[test]
fn test_detect_path_reads_file_and_uses_path_filename() {
    let detector = RepositoryMimeDetector::new().expect("default repository should load");
    let mut file = NamedTempFile::with_suffix(".pdf").expect("temp file should be created");
    std::io::Write::write_all(&mut file, b"%PDF-1.7\n").expect("temp file should be writable");

    let detected = detector
        .detect_path(file.path(), MimeDetectionPolicy::VerifyContent)
        .expect("path detection should succeed");

    assert_eq!(Some("application/pdf".to_owned()), detected);
}
