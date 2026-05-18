/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/

use std::io::Cursor;

use tempfile::NamedTempFile;

use qubit_mime::{
    CONFIG_MIME_MAX_BUFFER_SIZE,
    MimeConfig,
    MimeDetectionPolicy,
    MimeDetector,
    MimeDetectorBackend,
    MimeDetectorCore,
    MimeError,
    MimeResult,
    StreamBasedMimeDetector,
};

#[derive(Debug)]
struct PrefixDetector {
    core: MimeDetectorCore,
}

impl PrefixDetector {
    /// Creates a detector that recognizes one content prefix.
    fn new() -> Self {
        Self::with_core(MimeDetectorCore::default())
    }

    /// Creates a detector with explicit shared core settings.
    fn with_core(core: MimeDetectorCore) -> Self {
        Self { core }
    }
}

impl StreamBasedMimeDetector for PrefixDetector {
    /// Gets the shared detector core.
    fn core(&self) -> &MimeDetectorCore {
        &self.core
    }

    /// Gets the prefix length used for stream inputs.
    fn max_test_bytes(&self) -> usize {
        5
    }

    /// Returns no filename candidates.
    fn guess_from_filename(&self, _filename: &str) -> Vec<String> {
        Vec::new()
    }

    /// Recognizes the staged content prefix.
    fn guess_from_content_bytes(&self, content: &[u8]) -> MimeResult<Vec<String>> {
        if content == b"hello" {
            Ok(vec!["text/plain".to_owned()])
        } else {
            Ok(Vec::new())
        }
    }
}

/// Verifies a stream-based detector gets reader detection without backend boilerplate.
#[test]
fn test_detect_reader_uses_stream_based_defaults() {
    let detector = PrefixDetector::new();
    let mut reader = Cursor::new(b"hello world".to_vec());

    let detected = detector
        .detect_reader(&mut reader, None, MimeDetectionPolicy::VerifyContent)
        .expect("stream-based reader detection should succeed");

    assert_eq!(Some("text/plain".to_owned()), detected);
    assert_eq!(0, reader.position());
}

/// Verifies a stream-based detector gets local-file detection without backend boilerplate.
#[test]
fn test_detect_file_uses_stream_based_defaults() {
    let detector = PrefixDetector::new();
    let mut file = NamedTempFile::new().expect("temporary file should be created");
    std::io::Write::write_all(&mut file, b"hello world")
        .expect("temporary file should be writable");

    let detected = detector
        .detect_file(file.path(), MimeDetectionPolicy::VerifyContent)
        .expect("stream-based file detection should succeed");

    assert_eq!(Some("text/plain".to_owned()), detected);
}

#[test]
fn test_stream_based_backend_max_bytes_and_file_open_error_are_covered() {
    let detector = PrefixDetector::new();
    let missing_path =
        std::env::temp_dir().join(format!("qubit-mime-missing-{}", std::process::id()));

    assert_eq!(5, MimeDetectorBackend::max_test_bytes(&detector));
    assert!(
        StreamBasedMimeDetector::guess_from_file_stream(&detector, &missing_path).is_err(),
        "missing file should propagate the open error"
    );
}

#[test]
fn test_detect_reader_rejects_prefix_buffer_larger_than_configured_limit() {
    let mut config = qubit_config::Config::new();
    config
        .set(CONFIG_MIME_MAX_BUFFER_SIZE, 4_usize)
        .expect("maximum buffer size should be configurable");
    let detector = PrefixDetector::with_core(MimeDetectorCore::new(
        MimeConfig::from_config(&config)
            .expect("MIME config should parse with a custom maximum buffer size"),
    ));
    let mut reader = Cursor::new(b"hello world".to_vec());

    let error = detector
        .detect_reader(&mut reader, None, MimeDetectionPolicy::VerifyContent)
        .expect_err("oversized prefix allocation should be rejected");

    assert!(matches!(
        error,
        MimeError::BufferLimitExceeded {
            requested: 5,
            limit: 4,
        }
    ));
    assert_eq!(0, reader.position());
}
