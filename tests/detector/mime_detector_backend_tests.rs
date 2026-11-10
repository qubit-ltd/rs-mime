// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::Path;

use qubit_io::Output;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::options::LocalTempFileOptions;
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
