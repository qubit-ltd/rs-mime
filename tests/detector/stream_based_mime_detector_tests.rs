/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/

use std::io::{
    Cursor,
    Error,
    Read,
    Result as IoResult,
    Seek,
    SeekFrom,
};

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

struct ShortReadSeek {
    inner: Cursor<Vec<u8>>,
    max_chunk_size: usize,
}

impl ShortReadSeek {
    /// Creates a seekable reader that returns at most `max_chunk_size` bytes per read.
    fn new(content: &[u8], max_chunk_size: usize) -> Self {
        Self {
            inner: Cursor::new(content.to_vec()),
            max_chunk_size,
        }
    }

    /// Gets the current stream position.
    fn position(&self) -> u64 {
        self.inner.position()
    }
}

impl Read for ShortReadSeek {
    /// Reads at most the configured chunk size from the wrapped cursor.
    fn read(&mut self, buffer: &mut [u8]) -> IoResult<usize> {
        let requested = buffer.len().min(self.max_chunk_size);
        self.inner.read(&mut buffer[..requested])
    }
}

impl Seek for ShortReadSeek {
    /// Delegates seeking to the wrapped cursor.
    fn seek(&mut self, position: SeekFrom) -> IoResult<u64> {
        self.inner.seek(position)
    }
}

struct ReadErrorAfterPositionChange {
    position: u64,
}

impl ReadErrorAfterPositionChange {
    /// Creates a reader that moves position before returning a read error.
    fn new(position: u64) -> Self {
        Self { position }
    }

    /// Gets the current simulated stream position.
    fn position(&self) -> u64 {
        self.position
    }
}

impl Read for ReadErrorAfterPositionChange {
    /// Moves the stream position and reports a read failure.
    fn read(&mut self, _buffer: &mut [u8]) -> IoResult<usize> {
        self.position += 2;
        Err(Error::other("forced read failure"))
    }
}

impl Seek for ReadErrorAfterPositionChange {
    /// Supports position lookup and absolute restoration.
    fn seek(&mut self, position: SeekFrom) -> IoResult<u64> {
        match position {
            SeekFrom::Current(0) => Ok(self.position),
            SeekFrom::Start(position) => {
                self.position = position;
                Ok(position)
            }
            SeekFrom::Current(_) | SeekFrom::End(_) => {
                Err(Error::other("unsupported seek operation"))
            }
        }
    }
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

#[test]
fn test_detect_reader_reads_prefix_across_short_reads() {
    let detector = PrefixDetector::new();
    let mut reader = ShortReadSeek::new(b"hello world", 2);

    let detected = detector
        .detect_reader(&mut reader, None, MimeDetectionPolicy::VerifyContent)
        .expect("short-read stream detection should succeed");

    assert_eq!(Some("text/plain".to_owned()), detected);
    assert_eq!(0, reader.position());
}

#[test]
fn test_detect_reader_restores_position_after_read_error() {
    let detector = PrefixDetector::new();
    let mut reader = ReadErrorAfterPositionChange::new(3);

    let error = detector
        .detect_reader(&mut reader, None, MimeDetectionPolicy::VerifyContent)
        .expect_err("read failure should be reported");

    assert!(matches!(error, MimeError::Io(_)));
    assert_eq!(3, reader.position());
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
