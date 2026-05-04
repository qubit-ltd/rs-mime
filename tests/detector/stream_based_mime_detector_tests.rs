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
    MimeDetectionPolicy, MimeDetector, MimeDetectorCore, MimeResult, StreamBasedMimeDetector,
};

#[derive(Debug)]
struct PrefixDetector {
    core: MimeDetectorCore,
}

impl PrefixDetector {
    /// Creates a detector that recognizes one content prefix.
    fn new() -> Self {
        Self {
            core: MimeDetectorCore::default(),
        }
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
