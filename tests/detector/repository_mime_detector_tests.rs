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
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use qubit_config::Config;
use qubit_io::Output;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::options::LocalTempDirectoryOptions;
use qubit_local_files::options::LocalTempFileOptions;
use qubit_mime::CONFIG_MIME_AMBIGUOUS_MIME_MAPPING;
use qubit_mime::CONFIG_MIME_ENABLE_PRECISE_DETECTION;
use qubit_mime::CONFIG_MIME_PRECISE_DETECTION_PATTERNS;
use qubit_mime::MediaStreamClassifier;
use qubit_mime::MediaStreamType;
use qubit_mime::MimeConfig;
use qubit_mime::MimeDetectionPolicy;
use qubit_mime::MimeDetector;
use qubit_mime::MimeRepository;
use qubit_mime::MimeResult;
use qubit_mime::RepositoryMimeDetector;

#[derive(Debug)]
struct CountingAudioClassifier {
    calls: Arc<AtomicUsize>,
}

impl MediaStreamClassifier for CountingAudioClassifier {
    fn classify_file(&self, _file: &Path) -> MimeResult<MediaStreamType> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(MediaStreamType::AudioOnly)
    }

    fn classify_reader(&self, _reader: &mut dyn Read) -> MimeResult<MediaStreamType> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(MediaStreamType::AudioOnly)
    }
}

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
    let detector = RepositoryMimeDetector::new().expect("default repository should load");

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
    let detector = RepositoryMimeDetector::new().expect("default repository should load");

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
    let detector = RepositoryMimeDetector::new().expect("default repository should load");

    assert_eq!(
        Some("image/jpeg".to_owned()),
        detector
            .detect_bytes(b"%PDF-1.7\n", Some("photo.jpg"), MimeDetectionPolicy::PreferFilename,)
            .expect("combined detection should succeed")
    );
    assert_eq!(
        Some("application/pdf".to_owned()),
        detector
            .detect_bytes(b"%PDF-1.7\n", Some("photo.jpg"), MimeDetectionPolicy::VerifyContent,)
            .expect("combined detection should succeed")
    );
    assert_eq!(
        Some("application/pdf".to_owned()),
        detector
            .detect_bytes(b"%PDF-1.7\n", None, MimeDetectionPolicy::VerifyContent)
            .expect("combined detection should succeed")
    );
}

#[test]
fn test_prefix_and_complete_content_have_distinct_classifier_scopes() {
    let mut source = Config::new();
    source
        .set(CONFIG_MIME_ENABLE_PRECISE_DETECTION, true)
        .expect("precise detection should be configurable");
    let config = MimeConfig::from_config(&source).expect("MIME config should parse");
    let calls = Arc::new(AtomicUsize::new(0));
    let mut detector = RepositoryMimeDetector::from_mime_config(config.clone());
    detector
        .core_mut()
        .set_media_stream_classifier(Some(Arc::new(CountingAudioClassifier {
            calls: Arc::clone(&calls),
        })));
    let webm = b"\x1a\x45\xdf\xa3\x93\x42\x82\x88webm";

    assert_eq!(
        Some("video/webm".to_owned()),
        detector
            .detect_prefix(webm, Some("sample.webm"), MimeDetectionPolicy::VerifyContent)
            .expect("prefix detection should succeed"),
    );
    assert_eq!(0, calls.load(Ordering::SeqCst), "prefix must skip classifier");

    assert_eq!(
        Some("audio/webm".to_owned()),
        detector
            .detect_bytes(webm, Some("sample.webm"), MimeDetectionPolicy::VerifyContent)
            .expect("complete-content detection should succeed"),
    );
    assert_eq!(1, calls.load(Ordering::SeqCst), "complete content should classify once");

    let no_classifier = RepositoryMimeDetector::from_mime_config(config.clone());
    assert_eq!(
        Some("video/webm".to_owned()),
        no_classifier
            .detect_bytes(webm, Some("sample.webm"), MimeDetectionPolicy::VerifyContent)
            .expect("detection without a classifier should succeed"),
    );
    assert_eq!(
        1,
        calls.load(Ordering::SeqCst),
        "missing classifier must not call the injected one"
    );

    let mut disabled_source = Config::new();
    disabled_source
        .set(CONFIG_MIME_ENABLE_PRECISE_DETECTION, false)
        .expect("precise detection should be configurable");
    let disabled_config = MimeConfig::from_config(&disabled_source).expect("MIME config should parse");
    let mut disabled = RepositoryMimeDetector::from_mime_config(disabled_config);
    disabled
        .core_mut()
        .set_media_stream_classifier(Some(Arc::new(CountingAudioClassifier {
            calls: Arc::clone(&calls),
        })));
    assert_eq!(
        Some("video/webm".to_owned()),
        disabled
            .detect_bytes(webm, Some("sample.webm"), MimeDetectionPolicy::VerifyContent)
            .expect("detection with precise mode disabled should succeed"),
    );
    assert_eq!(
        1,
        calls.load(Ordering::SeqCst),
        "disabled precise detection must skip classifier"
    );

    assert_eq!(
        Some("application/pdf".to_owned()),
        detector
            .detect_bytes(webm, Some("report.pdf"), MimeDetectionPolicy::PreferFilename)
            .expect("unique filename should short-circuit content work"),
    );
    assert_eq!(1, calls.load(Ordering::SeqCst), "unique filename must skip classifier");
}

#[test]
fn test_filename_hint_cross_platform_path_refines_public_byte_and_reader_detection() {
    let repository = MimeRepository::from_xml(
        r#"<mime-info xmlns="http://www.freedesktop.org/standards/shared-mime-info">
  <mime-type type="video/webm">
    <magic priority="50"><match type="string" value="WEBM" offset="0"/></magic>
  </mime-type>
</mime-info>"#,
    )
    .expect("custom WEBM repository should parse");
    let mut source = Config::new();
    source
        .set(CONFIG_MIME_ENABLE_PRECISE_DETECTION, true)
        .expect("precise detection should be configurable");
    source
        .set(CONFIG_MIME_PRECISE_DETECTION_PATTERNS, "webm")
        .expect("precise patterns should be configurable");
    source
        .set(CONFIG_MIME_AMBIGUOUS_MIME_MAPPING, "webm:video/webm,audio/webm")
        .expect("ambiguous MIME mapping should be configurable");
    let config = MimeConfig::from_config(&source).expect("MIME config should parse");
    let calls = Arc::new(AtomicUsize::new(0));
    let mut detector = RepositoryMimeDetector::with_repository_and_config(&repository, config);
    detector
        .core_mut()
        .set_media_stream_classifier(Some(Arc::new(CountingAudioClassifier {
            calls: Arc::clone(&calls),
        })));
    let filename = r"C:\cache.v1\clip";
    let content = b"WEBM payload";

    assert_eq!(
        Some("audio/webm".to_owned()),
        detector
            .detect_bytes(content, Some(filename), MimeDetectionPolicy::VerifyContent)
            .expect("byte detection should refine the repository MIME")
    );
    assert_eq!(1, calls.load(Ordering::SeqCst));

    let mut reader = Cursor::new(content.to_vec());
    reader.set_position(2);
    assert_eq!(
        Some("audio/webm".to_owned()),
        detector
            .detect_reader(&mut reader, Some(filename), MimeDetectionPolicy::VerifyContent)
            .expect("reader detection should refine the repository MIME")
    );
    assert_eq!(2, calls.load(Ordering::SeqCst));
    assert_eq!(2, reader.position(), "reader position should be restored");
}

#[test]
fn test_detect_reader_does_not_consume_reader_position() {
    let detector = RepositoryMimeDetector::new().expect("default repository should load");
    let mut reader = Cursor::new(b"%PDF-1.7\n".to_vec());

    let detected = detector
        .detect_reader(&mut reader, Some("document.pdf"), MimeDetectionPolicy::VerifyContent)
        .expect("reader detection should succeed");

    assert_eq!(Some("application/pdf".to_owned()), detected);
    assert_eq!(0, reader.position());
}

#[test]
fn test_detect_file_reads_file_and_uses_file_name() {
    let detector = RepositoryMimeDetector::new().expect("default repository should load");
    let mut file = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_suffix(".pdf"))
        .expect("temp file should be created");
    Output::write_fully(&mut file, b"%PDF-1.7\n").expect("temp file should be writable");

    let detected = detector
        .detect_file(file.path(), MimeDetectionPolicy::VerifyContent)
        .expect("file detection should succeed");

    assert_eq!(Some("application/pdf".to_owned()), detected);
}

#[test]
#[cfg(unix)]
fn test_detect_file_follows_valid_symlink_and_rejects_directory_and_dangling_symlink() {
    let detector = RepositoryMimeDetector::new().expect("default repository should load");
    let filesystem = LocalFileSystem::host().expect("Host filesystem should open");
    let mut file = filesystem
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_suffix(".pdf"))
        .expect("PDF file should be created");
    Output::write_fully(&mut file, b"%PDF-1.7\n").expect("PDF content should be written");
    let directory = filesystem
        .create_temp_directory_with_options(&LocalTempDirectoryOptions::new())
        .expect("directory should be created");
    let valid_link = directory.path().join("valid-pdf-link");
    let dangling_link = directory.path().join("dangling-pdf-link");
    std::os::unix::fs::symlink(file.path(), &valid_link).expect("valid symlink should be created");
    std::os::unix::fs::symlink(directory.path().join("missing-target"), &dangling_link)
        .expect("dangling symlink should be created");

    assert_eq!(
        Some("application/pdf".to_owned()),
        detector
            .detect_file(&valid_link, MimeDetectionPolicy::VerifyContent)
            .expect("repository detector should follow a link to a regular PDF")
    );
    assert!(
        detector
            .detect_file(directory.path(), MimeDetectionPolicy::VerifyContent)
            .is_err(),
        "directory input should return an error"
    );
    assert!(
        detector
            .detect_file(&dangling_link, MimeDetectionPolicy::VerifyContent)
            .is_err(),
        "dangling symlink input should return an error"
    );
}

#[test]
fn test_accessors_empty_repository_and_reader_errors() {
    let repository = MimeRepository::empty();
    let config = MimeConfig::from_config(&Config::new()).expect("builtin config should parse");
    let mut detector = RepositoryMimeDetector::with_repository_and_config(&repository, config);

    detector.core_mut().set_media_stream_classifier(None);
    assert!(detector.core().media_stream_classifier().is_none());
    assert_eq!(0, detector.repository().all().len());
    assert_eq!(0, detector.guess_from_filename("unknown.bin").len());
    assert_eq!(0, detector.guess_from_content(b"unknown").len());
    assert_eq!(
        None,
        detector
            .detect_bytes(b"", Some("unknown.bin"), MimeDetectionPolicy::PreferFilename)
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
            .detect_reader(&mut seek_reader, None, MimeDetectionPolicy::VerifyContent)
            .is_err()
    );
    let detected = detector
        .detect_reader(&mut read_reader, None, MimeDetectionPolicy::VerifyContent)
        .expect("empty repositories should not read content bytes");
    assert_eq!(None, detected);
}
