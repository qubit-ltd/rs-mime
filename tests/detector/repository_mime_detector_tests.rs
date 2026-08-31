// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Tests for the repository-backed MIME detector.

use std::io::Cursor;
use std::io::Error;
use std::io::Read;
use std::io::Result as IoResult;
use std::io::Seek;
use std::io::SeekFrom;

use qubit_config::Config;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::options::LocalTempFileOptions;
use qubit_mime::MimeConfig;
use qubit_mime::MimeDetectionPolicy;
use qubit_mime::MimeRepository;
use qubit_mime::RepositoryMimeDetector;

#[derive(Debug, Clone, Copy)]
enum FailureMode {
    Seek,
    Read,
}

struct FailingReader {
    mode: FailureMode,
}

impl FailingReader {
    fn new(mode: FailureMode) -> Self {
        Self { mode }
    }
}

impl Read for FailingReader {
    fn read(&mut self, _buf: &mut [u8]) -> IoResult<usize> {
        match self.mode {
            FailureMode::Read => Err(Error::other("read failed")),
            FailureMode::Seek => Ok(0),
        }
    }
}

impl Seek for FailingReader {
    fn seek(&mut self, _pos: SeekFrom) -> IoResult<u64> {
        match self.mode {
            FailureMode::Seek => Err(Error::other("seek failed")),
            FailureMode::Read => Ok(0),
        }
    }
}

#[test]
fn test_detect_by_filename_uses_default_repository() {
    let detector =
        RepositoryMimeDetector::new().expect("default repository should load");

    assert_eq!(
        Some("image/jpeg".to_owned()),
        detector
            .detect_by_filename("photo.JPG")
            .expect("filename detection should succeed")
    );
    assert_eq!(
        Some("application/x-compressed-tar".to_owned()),
        detector
            .detect_by_filename("/tmp/archive.tar.gz")
            .expect("filename detection should succeed")
    );
    assert_eq!(
        None,
        detector
            .detect_by_filename("")
            .expect("filename detection should succeed")
    );
}

#[test]
fn test_detect_by_content_uses_default_repository_magic() {
    let detector =
        RepositoryMimeDetector::new().expect("default repository should load");

    assert_eq!(
        Some("application/pdf".to_owned()),
        detector
            .detect_by_content(b"%PDF-1.7\n")
            .expect("content detection should succeed")
    );
    assert_eq!(
        Some("image/png".to_owned()),
        detector
            .detect_by_content(b"\x89PNG\r\n\x1a\n")
            .expect("content detection should succeed")
    );
}

#[test]
fn test_detect_bytes_merges_filename_and_content_results() {
    let detector =
        RepositoryMimeDetector::new().expect("default repository should load");

    assert_eq!(
        Some("image/jpeg".to_owned()),
        detector
            .detect_bytes(
                b"%PDF-1.7\n",
                Some("photo.jpg"),
                MimeDetectionPolicy::PreferFilename,
            )
            .expect("combined detection should succeed")
    );
    assert_eq!(
        Some("application/pdf".to_owned()),
        detector
            .detect_bytes(
                b"%PDF-1.7\n",
                Some("photo.jpg"),
                MimeDetectionPolicy::VerifyContent,
            )
            .expect("combined detection should succeed")
    );
    assert_eq!(
        Some("application/pdf".to_owned()),
        detector
            .detect_bytes(
                b"%PDF-1.7\n",
                None,
                MimeDetectionPolicy::VerifyContent
            )
            .expect("combined detection should succeed")
    );
}

#[test]
fn test_detect_reader_does_not_consume_reader_position() {
    let detector =
        RepositoryMimeDetector::new().expect("default repository should load");
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
fn test_detect_file_reads_file_and_uses_file_name() {
    let detector =
        RepositoryMimeDetector::new().expect("default repository should load");
    let mut file = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .create_temp_file_with_options(
            &LocalTempFileOptions::new().with_suffix(".pdf"),
        )
        .expect("temp file should be created");
    std::io::Write::write_all(&mut file, b"%PDF-1.7\n")
        .expect("temp file should be writable");

    let detected = detector
        .detect_file(file.path(), MimeDetectionPolicy::VerifyContent)
        .expect("file detection should succeed");

    assert_eq!(Some("application/pdf".to_owned()), detected);
}

#[test]
fn test_accessors_empty_repository_and_reader_errors() {
    let repository = MimeRepository::empty();
    let config = MimeConfig::from_config(&Config::new())
        .expect("builtin config should parse");
    let mut detector =
        RepositoryMimeDetector::with_repository_and_config(&repository, config);

    detector.core_mut().set_media_stream_classifier(None);
    assert!(detector.core().media_stream_classifier().is_none());
    assert_eq!(0, detector.repository().all().len());
    assert_eq!(0, detector.guess_from_filename("unknown.bin").len());
    assert_eq!(0, detector.guess_from_content(b"unknown").len());
    assert_eq!(
        None,
        detector
            .detect_bytes(
                b"",
                Some("unknown.bin"),
                MimeDetectionPolicy::PreferFilename
            )
            .expect("combined detection should succeed")
    );

    let mut seek_reader = FailingReader::new(FailureMode::Seek);
    let mut read_reader = FailingReader::new(FailureMode::Read);
    let mut buffer = [];
    assert_eq!(
        0,
        seek_reader
            .read(&mut buffer)
            .expect("seek-mode reader should allow reads")
    );
    assert!(
        detector
            .detect_reader(
                &mut seek_reader,
                None,
                MimeDetectionPolicy::VerifyContent
            )
            .is_err()
    );
    let detected = detector
        .detect_reader(
            &mut read_reader,
            None,
            MimeDetectionPolicy::VerifyContent,
        )
        .expect("empty repositories should not read content bytes");
    assert_eq!(None, detected);
}
