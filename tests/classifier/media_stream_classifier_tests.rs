// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fs;
use std::io::Cursor;
use std::io::Error;
use std::io::Read;
use std::io::Result as IoResult;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::options::LocalTempDirectoryOptions;
use qubit_mime::FileBasedMediaStreamClassifier;
use qubit_mime::MediaStreamClassifier;
use qubit_mime::MediaStreamClassifierBackend;
use qubit_mime::MediaStreamType;
use qubit_mime::MimeError;
use qubit_mime::MimeResult;

#[derive(Debug)]
struct StaticClassifier {
    stream_type: MediaStreamType,
}

impl MediaStreamClassifier for StaticClassifier {
    fn classify_file(&self, _file: &Path) -> MimeResult<MediaStreamType> {
        Ok(self.stream_type)
    }

    fn classify_reader(&self, _reader: &mut dyn Read) -> MimeResult<MediaStreamType> {
        Ok(self.stream_type)
    }
}

#[derive(Debug)]
struct BackendClassifier;

impl MediaStreamClassifierBackend for BackendClassifier {
    fn classify_by_local_file(&self, _file: &Path) -> MimeResult<MediaStreamType> {
        Ok(MediaStreamType::VideoOnly)
    }

    fn classify_by_content(&self, reader: &mut dyn Read) -> MimeResult<MediaStreamType> {
        let mut content = Vec::new();
        reader.read_to_end(&mut content)?;
        if content == b"audio" {
            Ok(MediaStreamType::AudioOnly)
        } else {
            Ok(MediaStreamType::None)
        }
    }
}

#[derive(Debug)]
struct LocalFileOnlyClassifier;

impl FileBasedMediaStreamClassifier for LocalFileOnlyClassifier {
    fn classify_by_local_file(&self, file: &Path) -> MimeResult<MediaStreamType> {
        if file.is_file() {
            Ok(MediaStreamType::VideoWithAudio)
        } else {
            Ok(MediaStreamType::None)
        }
    }
}

#[derive(Debug)]
struct LimitedLocalFileOnlyClassifier {
    max_staging_size: u64,
}

impl FileBasedMediaStreamClassifier for LimitedLocalFileOnlyClassifier {
    fn classify_by_local_file(&self, file: &Path) -> MimeResult<MediaStreamType> {
        if file.is_file() {
            Ok(MediaStreamType::VideoWithAudio)
        } else {
            Ok(MediaStreamType::None)
        }
    }

    fn max_staging_size(&self) -> u64 {
        self.max_staging_size
    }
}

#[derive(Debug)]
struct PathRecordingFileClassifier {
    seen_path: Mutex<Option<PathBuf>>,
}

impl PathRecordingFileClassifier {
    /// Creates a classifier that records staged local file paths.
    fn new() -> Self {
        Self {
            seen_path: Mutex::new(None),
        }
    }

    /// Gets the last staged path.
    fn seen_path(&self) -> Option<PathBuf> {
        self.seen_path
            .lock()
            .expect("path recorder lock should not be poisoned")
            .clone()
    }
}

impl FileBasedMediaStreamClassifier for PathRecordingFileClassifier {
    fn classify_by_local_file(&self, file: &Path) -> MimeResult<MediaStreamType> {
        *self
            .seen_path
            .lock()
            .expect("path recorder lock should not be poisoned") = Some(file.to_path_buf());
        Ok(MediaStreamType::VideoOnly)
    }
}

#[derive(Debug)]
struct FailingLocalFileOnlyClassifier;

impl FileBasedMediaStreamClassifier for FailingLocalFileOnlyClassifier {
    fn classify_by_local_file(&self, _file: &Path) -> MimeResult<MediaStreamType> {
        Err(MimeError::InvalidClassifierInput {
            reason: "forced".to_owned(),
        })
    }
}

struct ErrorReader;

impl Read for ErrorReader {
    fn read(&mut self, _buf: &mut [u8]) -> IoResult<usize> {
        Err(Error::other("forced read error"))
    }
}

struct BufferLimitedReader {
    remaining: usize,
    max_buffer_len: usize,
}

impl BufferLimitedReader {
    /// Creates a reader that fails when callers request overly large buffers.
    fn new(remaining: usize, max_buffer_len: usize) -> Self {
        Self {
            remaining,
            max_buffer_len,
        }
    }
}

impl Read for BufferLimitedReader {
    fn read(&mut self, buf: &mut [u8]) -> IoResult<usize> {
        if buf.len() > self.max_buffer_len {
            return Err(Error::other(format!(
                "buffer too large: {} > {}",
                buf.len(),
                self.max_buffer_len,
            )));
        }
        if self.remaining == 0 {
            return Ok(0);
        }
        let bytes_to_write = self.remaining.min(buf.len());
        buf[..bytes_to_write].fill(b'm');
        self.remaining -= bytes_to_write;
        Ok(bytes_to_write)
    }
}

#[test]
fn test_media_stream_classifier_trait_supports_content_classification() {
    let classifier = StaticClassifier {
        stream_type: MediaStreamType::AudioOnly,
    };

    assert_eq!(
        MediaStreamType::AudioOnly,
        classifier
            .classify_content(b"audio")
            .expect("classification should succeed")
    );
}

#[test]
fn test_backend_classifier_gets_default_content_and_file_entries() {
    let classifier = BackendClassifier;

    assert_eq!(
        MediaStreamType::AudioOnly,
        classifier
            .classify_content(b"audio")
            .expect("content classification should use backend content method")
    );
    assert_eq!(
        MediaStreamType::VideoOnly,
        classifier
            .classify_file(Path::new("Cargo.toml"))
            .expect("file classification should use backend local-file method")
    );
    assert!(matches!(
        classifier.classify_file(Path::new(".")),
        Err(MimeError::InvalidClassifierInput { .. })
    ));
    assert!(classifier.classify_file(Path::new("__missing_media__")).is_err());
}

#[test]
fn test_file_based_classifier_stages_content_to_local_file() {
    let classifier = LocalFileOnlyClassifier;

    assert_eq!(
        MediaStreamType::VideoWithAudio,
        classifier
            .classify_content(b"media")
            .expect("content should be staged to a temporary file")
    );
}

#[test]
fn test_file_based_classifier_uses_non_predictable_temporary_file_name() {
    let classifier = PathRecordingFileClassifier::new();

    let classified = classifier
        .classify_content(b"media")
        .expect("content should be staged to a temporary file");

    assert_eq!(MediaStreamType::VideoOnly, classified);
    let staged_path = classifier.seen_path().expect("staged path should be recorded");
    let filename = staged_path
        .file_name()
        .and_then(|name| name.to_str())
        .expect("staged filename should be UTF-8");
    let predictable_prefix = format!("FileBasedMediaStreamClassifier-{}-", std::process::id(),);
    assert!(
        filename.starts_with("FileBasedMediaStreamClassifier-"),
        "staged temp filename should preserve the configured prefix: {filename}",
    );
    assert!(
        filename.ends_with(".tmp"),
        "staged temp filename should preserve the configured suffix: {filename}",
    );
    assert!(
        !filename.starts_with(&predictable_prefix),
        "staged temp filename should not use a predictable pid/counter pattern: {filename}",
    );
}

#[test]
fn test_file_based_classifier_streams_reader_to_temporary_file_in_bounded_chunks() {
    let classifier = LocalFileOnlyClassifier;
    let mut reader = BufferLimitedReader::new(256 * 1024, 16 * 1024);

    let classified = classifier
        .classify_reader(&mut reader)
        .expect("reader should be staged without requesting oversized buffers");

    assert_eq!(MediaStreamType::VideoWithAudio, classified);
}

#[test]
fn test_file_based_classifier_rejects_reader_exceeding_staging_limit() {
    let classifier = LimitedLocalFileOnlyClassifier { max_staging_size: 4 };
    let mut reader = Cursor::new(b"media".to_vec());

    let error = classifier
        .classify_reader(&mut reader)
        .expect_err("oversized reader should be rejected before classification");

    assert!(matches!(
        error,
        MimeError::InvalidClassifierInput {
            reason,
        } if reason.contains("staging limit") && reason.contains("4")
    ));
}

#[test]
fn test_file_based_classifier_reports_temporary_file_creation_error() {
    const CHILD_ENV: &str = "QUBIT_MIME_CHECK_CLASSIFIER_TEMPFILE_ERROR";
    const TEST_NAME: &str =
        "classifier::media_stream_classifier_tests::test_file_based_classifier_reports_temporary_file_creation_error";

    if std::env::var_os(CHILD_ENV).is_some() {
        let classifier = LocalFileOnlyClassifier;

        let error = classifier
            .classify_content(b"media")
            .expect_err("invalid temporary directory should fail content classification");

        assert!(error.to_string().contains("I/O error"));
        return;
    }

    let temp_dir = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .create_temp_directory_with_options(
            &LocalTempDirectoryOptions::new().with_prefix("qubit-mime-classifier-error-"),
        )
        .expect("temporary parent directory should be created");
    let invalid_temp_dir = temp_dir.path().join("not-a-directory");
    fs::write(&invalid_temp_dir, b"not a directory")
        .expect("invalid temporary directory placeholder should be created");
    let output =
        std::process::Command::new(std::env::current_exe().expect("current test binary path should be available"))
            .arg(TEST_NAME)
            .arg("--exact")
            .arg("--nocapture")
            .arg("--test-threads=1")
            .env(CHILD_ENV, "1")
            .env("TMPDIR", invalid_temp_dir)
            .output()
            .expect("child test process should run");

    assert!(
        output.status.success(),
        "child test failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn test_file_based_classifier_creates_missing_temporary_directory() {
    const CHILD_ENV: &str = "QUBIT_MIME_CHECK_CLASSIFIER_MISSING_TMPDIR";
    const TEST_NAME: &str =
        "classifier::media_stream_classifier_tests::test_file_based_classifier_creates_missing_temporary_directory";

    if std::env::var_os(CHILD_ENV).is_some() {
        let classifier = LocalFileOnlyClassifier;

        let classified = classifier
            .classify_content(b"media")
            .expect("missing temporary directory should be created");

        assert_eq!(MediaStreamType::VideoWithAudio, classified);
        return;
    }

    let temp_dir = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .create_temp_directory_with_options(
            &LocalTempDirectoryOptions::new().with_prefix("qubit-mime-classifier-missing-"),
        )
        .expect("temporary parent directory should be created");
    let missing_temp_dir = temp_dir.path().join("missing").join("nested");
    let output =
        std::process::Command::new(std::env::current_exe().expect("current test binary path should be available"))
            .arg(TEST_NAME)
            .arg("--exact")
            .arg("--nocapture")
            .arg("--test-threads=1")
            .env(CHILD_ENV, "1")
            .env("TMPDIR", &missing_temp_dir)
            .output()
            .expect("child test process should run");

    assert!(
        output.status.success(),
        "child test failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        missing_temp_dir.is_dir(),
        "missing temporary directory should be created"
    );
}

#[test]
fn test_file_based_classifier_propagates_local_file_error() {
    let classifier = FailingLocalFileOnlyClassifier;
    let mut error_reader = ErrorReader;

    assert!(matches!(
        classifier.classify_content(b"media"),
        Err(MimeError::InvalidClassifierInput { .. })
    ));
    assert!(classifier.classify_reader(&mut error_reader).is_err());
}

#[test]
fn test_boxed_media_stream_classifier_trait_object_delegates_all_entry_points() {
    let classifier: Box<dyn MediaStreamClassifier> = Box::new(StaticClassifier {
        stream_type: MediaStreamType::VideoWithAudio,
    });

    assert_media_stream_classifier_delegates(&classifier, MediaStreamType::VideoWithAudio);
}

#[test]
fn test_shared_media_stream_classifier_trait_object_delegates_all_entry_points() {
    let classifier: Arc<dyn MediaStreamClassifier> = Arc::new(StaticClassifier {
        stream_type: MediaStreamType::VideoOnly,
    });
    let cloned = classifier.clone();

    assert_media_stream_classifier_delegates(&classifier, MediaStreamType::VideoOnly);
    assert_media_stream_classifier_delegates(&cloned, MediaStreamType::VideoOnly);
}

/// Asserts that a concrete classifier handle implements and delegates the
/// trait.
fn assert_media_stream_classifier_delegates<C>(classifier: &C, expected: MediaStreamType)
where
    C: MediaStreamClassifier,
{
    let mut reader = Cursor::new(b"media".to_vec());

    assert_eq!(
        expected,
        classifier
            .classify_file(Path::new("Cargo.toml"))
            .expect("trait-bound file classification should delegate")
    );
    assert_eq!(
        expected,
        classifier
            .classify_reader(&mut reader)
            .expect("trait-bound reader classification should delegate")
    );
    assert_eq!(
        expected,
        classifier
            .classify_content(b"media")
            .expect("trait-bound content classification should delegate")
    );
}
