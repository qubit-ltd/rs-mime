// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;
use std::path::Path;
use std::sync::Arc;

use qubit_config::Config;
use qubit_io::Output;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::options::LocalTempFileOptions;
use qubit_mime::ContentRequirement;
use qubit_mime::MediaStreamClassifier;
use qubit_mime::MediaStreamType;
use qubit_mime::MimeConfig;
use qubit_mime::MimeDetectionPolicy;
use qubit_mime::MimeDetector;
use qubit_mime::MimeDetectorBackend;
use qubit_mime::MimeDetectorCore;
use qubit_mime::MimeError;
use qubit_mime::MimeResult;

#[derive(Debug)]
struct Backend {
    core: MimeDetectorCore,
}

#[derive(Debug)]
struct FailingBackend {
    core: MimeDetectorCore,
}

#[derive(Debug)]
struct ReaderScopeBackend {
    core: MimeDetectorCore,
}

#[derive(Debug)]
struct WholeResourceClassifier;

struct SeekFailReader {
    inner: std::io::Cursor<Vec<u8>>,
    failed_positions: Vec<u64>,
}

struct ReadFailReader {
    position: u64,
}

#[derive(Debug)]
struct FailingWholeResourceClassifier;

impl Read for SeekFailReader {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.inner.read(buffer)
    }
}

impl Seek for SeekFailReader {
    fn seek(&mut self, from: SeekFrom) -> std::io::Result<u64> {
        if let SeekFrom::Start(position) = from
            && self.failed_positions.contains(&position)
        {
            return Err(std::io::Error::other(format!("seek to {position} failed")));
        }
        self.inner.seek(from)
    }
}

impl Read for ReadFailReader {
    fn read(&mut self, _buffer: &mut [u8]) -> std::io::Result<usize> {
        self.position += 1;
        Err(std::io::Error::other("read failed"))
    }
}

impl Seek for ReadFailReader {
    fn seek(&mut self, from: SeekFrom) -> std::io::Result<u64> {
        match from {
            SeekFrom::Start(position) => {
                self.position = position;
                Ok(position)
            }
            SeekFrom::Current(0) => Ok(self.position),
            SeekFrom::Current(_) | SeekFrom::End(_) => Err(std::io::Error::other("unsupported seek")),
        }
    }
}

impl MediaStreamClassifier for WholeResourceClassifier {
    fn classify_file(&self, _file: &Path) -> Result<MediaStreamType, MimeError> {
        Ok(MediaStreamType::AudioOnly)
    }

    fn classify_reader(&self, reader: &mut dyn Read) -> Result<MediaStreamType, MimeError> {
        let mut first_byte = [0_u8; 1];
        reader.read_exact(&mut first_byte)?;
        if first_byte[0] != b'w' {
            return Err(MimeError::InvalidClassifierInput {
                reason: "classifier did not receive the whole resource".to_owned(),
            });
        }
        Ok(MediaStreamType::AudioOnly)
    }
}

impl MediaStreamClassifier for FailingWholeResourceClassifier {
    fn classify_file(&self, _file: &Path) -> Result<MediaStreamType, MimeError> {
        Err(MimeError::InvalidClassifierInput {
            reason: "forced classifier failure".to_owned(),
        })
    }

    fn classify_reader(&self, _reader: &mut dyn Read) -> Result<MediaStreamType, MimeError> {
        Err(MimeError::InvalidClassifierInput {
            reason: "forced classifier failure".to_owned(),
        })
    }
}

impl FailingBackend {
    /// Creates a backend that fails every content inspection.
    fn new() -> Self {
        Self {
            core: MimeDetectorCore::from_mime_config(MimeConfig::default()),
        }
    }
}

impl Backend {
    fn new() -> Self {
        Self {
            core: MimeDetectorCore::from_mime_config(MimeConfig::default()),
        }
    }
}

impl ReaderScopeBackend {
    fn new() -> Self {
        let mut config = Config::new();
        config
            .set(qubit_mime::CONFIG_MIME_ENABLE_PRECISE_DETECTION, true)
            .expect("precise detection should be configurable");
        config
            .set(qubit_mime::CONFIG_MIME_PRECISE_DETECTION_PATTERNS, "webm")
            .expect("precise detection patterns should be configurable");
        config
            .set(
                qubit_mime::CONFIG_MIME_AMBIGUOUS_MIME_MAPPING,
                "webm:video/webm,audio/webm",
            )
            .expect("ambiguous MIME mapping should be configurable");
        let mut core = MimeDetectorCore::new(MimeConfig::from_config(&config).expect("config should parse"));
        core.set_media_stream_classifier(Some(Arc::new(WholeResourceClassifier)));
        Self { core }
    }
}

impl MimeDetectorBackend for Backend {
    fn core(&self) -> &MimeDetectorCore {
        &self.core
    }

    fn max_test_bytes(&self) -> usize {
        16
    }

    fn guess_from_filename(&self, filename: &str) -> Vec<String> {
        filename
            .ends_with(".txt")
            .then(|| "text/plain".to_owned())
            .into_iter()
            .collect()
    }

    fn guess_from_content(&self, content: &[u8]) -> MimeResult<Vec<String>> {
        Ok(content
            .starts_with(b"%PDF")
            .then(|| "application/pdf".to_owned())
            .into_iter()
            .collect())
    }
}

impl MimeDetectorBackend for FailingBackend {
    fn core(&self) -> &MimeDetectorCore {
        &self.core
    }

    fn max_test_bytes(&self) -> usize {
        16
    }

    fn guess_from_filename(&self, _filename: &str) -> Vec<String> {
        Vec::new()
    }

    fn guess_from_content(&self, _content: &[u8]) -> MimeResult<Vec<String>> {
        Err(MimeError::detector_backend("failing", "forced failure"))
    }
}

impl MimeDetectorBackend for ReaderScopeBackend {
    fn core(&self) -> &MimeDetectorCore {
        &self.core
    }

    fn max_test_bytes(&self) -> usize {
        4
    }

    fn guess_from_filename(&self, _filename: &str) -> Vec<String> {
        Vec::new()
    }

    fn guess_from_content(&self, content: &[u8]) -> MimeResult<Vec<String>> {
        Ok(content
            .starts_with(b"webm")
            .then(|| "video/webm".to_owned())
            .into_iter()
            .collect())
    }
}

#[test]
fn test_top_level_reader_detection_uses_whole_resource_for_backend_and_classifier() {
    let backend = ReaderScopeBackend::new();
    let mut reader = std::io::Cursor::new(b"webmTAIL".to_vec());
    reader.set_position(4);

    let detected = backend
        .detect_reader(&mut reader, None, MimeDetectionPolicy::VerifyContent)
        .expect("whole-resource reader detection should succeed");

    assert_eq!(Some("audio/webm".to_owned()), detected);
    assert_eq!(4, reader.position(), "detection should restore the input position");
}

#[test]
fn test_top_level_reader_detection_restores_position_after_backend_error() {
    let backend = FailingBackend::new();
    let mut reader = std::io::Cursor::new(b"content".to_vec());
    reader.set_position(4);

    assert!(matches!(
        backend.detect_reader(&mut reader, None, MimeDetectionPolicy::VerifyContent),
        Err(MimeError::DetectorBackend { .. })
    ));
    assert_eq!(4, reader.position());
}

#[test]
fn test_top_level_reader_detection_restores_position_after_read_error() {
    let backend = Backend::new();
    let mut reader = ReadFailReader { position: 4 };

    let error = backend
        .detect_reader(&mut reader, None, MimeDetectionPolicy::VerifyContent)
        .expect_err("reader failure should be reported");

    assert!(matches!(error, MimeError::Io(ref error) if error.to_string() == "read failed"));
    assert_eq!(4, reader.position);
}

#[test]
fn test_top_level_reader_detection_reports_empty_and_eof_windows() {
    let backend = Backend::new();
    let mut empty = std::io::Cursor::new(Vec::new());
    assert_eq!(
        None,
        backend
            .detect_reader(&mut empty, None, MimeDetectionPolicy::VerifyContent)
            .expect("empty input should be a valid detection result")
    );
    assert_eq!(0, empty.position());

    let mut at_eof = std::io::Cursor::new(b"%PDF-1.7".to_vec());
    at_eof.set_position(8);
    assert_eq!(
        Some("application/pdf".to_owned()),
        backend
            .detect_reader(&mut at_eof, None, MimeDetectionPolicy::VerifyContent)
            .expect("EOF cursor should still detect the whole resource")
    );
    assert_eq!(8, at_eof.position());
}

#[test]
fn test_top_level_reader_detection_restores_after_classifier_error() {
    let mut backend = ReaderScopeBackend::new();
    backend
        .core
        .set_media_stream_classifier(Some(Arc::new(FailingWholeResourceClassifier)));
    let mut reader = std::io::Cursor::new(b"webmTAIL".to_vec());
    reader.set_position(4);

    let error = backend
        .detect_reader(&mut reader, None, MimeDetectionPolicy::VerifyContent)
        .expect_err("classifier failure should be propagated");

    assert!(matches!(error, MimeError::InvalidClassifierInput { .. }));
    assert_eq!(4, reader.position());
}

#[test]
fn test_top_level_reader_restore_error_precedes_successful_detection() {
    let backend = Backend::new();
    let mut reader = SeekFailReader {
        inner: std::io::Cursor::new(b"%PDF-1.7".to_vec()),
        failed_positions: vec![4],
    };
    reader.inner.set_position(4);

    let error = backend
        .detect_reader(&mut reader, None, MimeDetectionPolicy::VerifyContent)
        .expect_err("restoring after successful detection should fail");

    assert!(matches!(error, MimeError::Io(ref error) if error.to_string() == "seek to 4 failed"));
}

#[test]
fn test_top_level_reader_restore_error_precedes_classifier_error() {
    let mut backend = ReaderScopeBackend::new();
    backend
        .core
        .set_media_stream_classifier(Some(Arc::new(FailingWholeResourceClassifier)));
    let mut reader = SeekFailReader {
        inner: std::io::Cursor::new(b"webmTAIL".to_vec()),
        failed_positions: vec![4],
    };
    reader.inner.set_position(4);

    let error = backend
        .detect_reader(&mut reader, None, MimeDetectionPolicy::VerifyContent)
        .expect_err("restore error should take precedence over classifier failure");

    assert!(matches!(error, MimeError::Io(ref error) if error.to_string() == "seek to 4 failed"));
}

#[test]
fn test_top_level_reader_detection_restores_after_initial_seek_error() {
    let backend = Backend::new();
    let mut reader = SeekFailReader {
        inner: std::io::Cursor::new(b"content".to_vec()),
        failed_positions: vec![0],
    };
    reader.inner.set_position(4);

    let error = backend
        .detect_reader(&mut reader, None, MimeDetectionPolicy::VerifyContent)
        .expect_err("initial seek to the resource start should fail");

    assert!(matches!(error, MimeError::Io(ref error) if error.to_string() == "seek to 0 failed"));
    assert_eq!(4, reader.inner.position());
}

#[test]
fn test_top_level_reader_restore_error_takes_precedence_over_initial_seek_error() {
    let backend = Backend::new();
    let mut reader = SeekFailReader {
        inner: std::io::Cursor::new(b"content".to_vec()),
        failed_positions: vec![0, 4],
    };
    reader.inner.set_position(4);

    let error = backend
        .detect_reader(&mut reader, None, MimeDetectionPolicy::VerifyContent)
        .expect_err("both initial seek and restoration should fail");

    assert!(matches!(error, MimeError::Io(ref error) if error.to_string() == "seek to 4 failed"));
}

#[test]
fn test_mime_detector_backend_blanket_impl_uses_policy_and_sources() {
    let backend = Backend::new();

    assert_eq!(
        Some("text/plain".to_owned()),
        backend
            .detect_by_filename("note.txt")
            .expect("filename detection should succeed")
    );
    assert_eq!(
        Some("application/pdf".to_owned()),
        backend
            .detect_by_content(b"%PDF-1.7")
            .expect("content detection should succeed")
    );
    assert_eq!(
        Some("text/plain".to_owned()),
        backend
            .detect(b"%PDF-1.7", Some("note.txt"), MimeDetectionPolicy::PreferFilename)
            .expect("combined detection should succeed"),
    );
    assert_eq!(
        Some("application/pdf".to_owned()),
        backend
            .detect_file(
                Path::new("tests/fixtures/real_files/test.pdf"),
                MimeDetectionPolicy::VerifyContent
            )
            .unwrap(),
    );
}

#[test]
fn test_mime_detector_backend_propagates_content_errors() {
    let backend = FailingBackend::new();

    let content_error = backend
        .detect_by_content(b"content")
        .expect_err("content detection must retain backend failures");
    let combined_error = backend
        .detect(b"content", Some("content.bin"), MimeDetectionPolicy::VerifyContent)
        .expect_err("combined detection must retain backend failures");

    assert!(matches!(
        content_error,
        MimeError::DetectorBackend { ref backend, .. } if backend == "failing"
    ));
    assert!(matches!(
        combined_error,
        MimeError::DetectorBackend { ref backend, .. } if backend == "failing"
    ));
}

#[derive(Debug)]
struct CompleteDefaultBackend(Backend);

#[derive(Debug)]
struct DeclaredPrefixBackend {
    core: MimeDetectorCore,
    declared: usize,
    legacy: usize,
}

impl MimeDetectorBackend for DeclaredPrefixBackend {
    fn core(&self) -> &MimeDetectorCore {
        &self.core
    }
    fn max_test_bytes(&self) -> usize {
        self.legacy
    }
    fn content_requirement(&self) -> ContentRequirement {
        ContentRequirement::Prefix(self.declared)
    }
    fn guess_from_filename(&self, _filename: &str) -> Vec<String> {
        Vec::new()
    }
    fn guess_from_content(&self, content: &[u8]) -> MimeResult<Vec<String>> {
        Ok(vec![format!("application/x-length-{}", content.len())])
    }
}

#[test]
fn test_default_reader_uses_declared_prefix_not_legacy_length() {
    for (declared, legacy) in [(4, 8), (8, 4)] {
        let backend = DeclaredPrefixBackend {
            core: MimeDetectorCore::from_mime_config(MimeConfig::default()),
            declared,
            legacy,
        };
        let mut reader = std::io::Cursor::new(b"0123456789abcdef".to_vec());
        reader.set_position(3);
        let (candidates, content) = backend.guess_from_reader(&mut reader).unwrap();
        assert_eq!(content.len(), declared);
        assert_eq!(candidates, [format!("application/x-length-{declared}")]);
        assert_eq!(reader.position(), 3);
    }
}

#[test]
fn test_default_reader_declared_prefix_budget_and_eof_edges() {
    let make_backend = |declared| {
        let mut config = Config::new();
        config.set(qubit_mime::CONFIG_MIME_MAX_BUFFER_SIZE, 4_u64).unwrap();
        DeclaredPrefixBackend {
            core: MimeDetectorCore::new(MimeConfig::from_config(&config).unwrap()),
            declared,
            legacy: 8,
        }
    };
    let exact = make_backend(4);
    let mut reader = std::io::Cursor::new(b"01234567".to_vec());
    assert_eq!(exact.guess_from_reader(&mut reader).unwrap().1, b"0123");

    let over = make_backend(5);
    let reads = std::rc::Rc::new(std::cell::Cell::new(0));
    let mut reader = CountedReader {
        cursor: std::io::Cursor::new(b"01234567".to_vec()),
        reads: std::rc::Rc::clone(&reads),
    };
    assert!(matches!(
        over.guess_from_reader(&mut reader),
        Err(MimeError::BufferLimitExceeded { requested: 5, limit: 4 })
    ));
    assert_eq!(reads.get(), 0, "over-budget prefix must not read");

    let zero = make_backend(0);
    let mut reader = std::io::Cursor::new(b"0123".to_vec());
    assert!(zero.guess_from_reader(&mut reader).unwrap().1.is_empty());
    let short = make_backend(4);
    let mut reader = std::io::Cursor::new(b"xy".to_vec());
    assert_eq!(short.guess_from_reader(&mut reader).unwrap().1, b"xy");
}

impl MimeDetectorBackend for CompleteDefaultBackend {
    fn core(&self) -> &MimeDetectorCore {
        self.0.core()
    }
    fn max_test_bytes(&self) -> usize {
        5
    }
    fn content_requirement(&self) -> qubit_mime::ContentRequirement {
        qubit_mime::ContentRequirement::Complete
    }
    fn guess_from_filename(&self, filename: &str) -> Vec<String> {
        self.0.guess_from_filename(filename)
    }
    fn guess_from_content(&self, content: &[u8]) -> MimeResult<Vec<String>> {
        self.0.guess_from_content(content)
    }
}

#[derive(Debug)]
struct CountedReader {
    cursor: std::io::Cursor<Vec<u8>>,
    reads: std::rc::Rc<std::cell::Cell<usize>>,
}

impl std::io::Read for CountedReader {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.reads.set(self.reads.get() + 1);
        std::io::Read::read(&mut self.cursor, buffer)
    }
}
impl std::io::Seek for CountedReader {
    fn seek(&mut self, from: std::io::SeekFrom) -> std::io::Result<u64> {
        std::io::Seek::seek(&mut self.cursor, from)
    }
}

#[test]
fn test_default_complete_reader_hook_rejects_without_reading() {
    let backend = CompleteDefaultBackend(Backend::new());
    let reads = std::rc::Rc::new(std::cell::Cell::new(0));
    let mut reader = CountedReader {
        cursor: std::io::Cursor::new(b"%PDF-more-than-five-bytes".to_vec()),
        reads: std::rc::Rc::clone(&reads),
    };
    assert!(matches!(
        backend.guess_from_reader(&mut reader),
        Err(MimeError::CompleteContentRequired)
    ));
    assert_eq!(0, reads.get());
    assert_eq!(0, std::io::Seek::stream_position(&mut reader).unwrap());

    let adapter = qubit_mime::MimeDetectorAdapter::new(CompleteDefaultBackend(Backend::new()));
    let mut reader = std::io::Cursor::new(b"%PDF-more-than-five-bytes".to_vec());
    assert!(matches!(
        qubit_mime::MimeContentBackend::detect_reader(&adapter, &mut reader),
        Err(MimeError::CompleteContentRequired)
    ));
    assert_eq!(0, reader.position());

    let mut file = LocalFileSystem::host()
        .expect("host filesystem should open")
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_suffix(".pdf"))
        .expect("temporary PDF should be created");
    Output::write_fully(&mut file, b"%PDF-1.7\n").expect("temporary PDF should be writable");
    assert!(matches!(
        backend.guess_from_file(file.path()),
        Err(MimeError::CompleteContentRequired)
    ));
}

#[test]
fn test_complete_reader_detection_propagates_error_and_preserves_filename_fast_path() {
    let backend = CompleteDefaultBackend(Backend::new());
    let reads = std::rc::Rc::new(std::cell::Cell::new(0));
    let mut reader = CountedReader {
        cursor: std::io::Cursor::new(b"%PDF-more-than-five-bytes".to_vec()),
        reads: std::rc::Rc::clone(&reads),
    };
    reader.cursor.set_position(4);

    assert!(matches!(
        backend.detect_reader(&mut reader, Some("note.txt"), MimeDetectionPolicy::VerifyContent),
        Err(MimeError::CompleteContentRequired)
    ));
    assert_eq!(0, reads.get(), "Complete rejection must not read the input");
    assert_eq!(4, std::io::Seek::stream_position(&mut reader).unwrap());

    assert_eq!(
        Some("text/plain".to_owned()),
        backend
            .detect_reader(&mut reader, Some("note.txt"), MimeDetectionPolicy::PreferFilename)
            .expect("PreferFilename should return the unique filename candidate")
    );
    assert_eq!(0, reads.get(), "PreferFilename must skip content reading");
    assert_eq!(4, std::io::Seek::stream_position(&mut reader).unwrap());
}
