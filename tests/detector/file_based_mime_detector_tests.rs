// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Mutex;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::LocalTempDirectoryOptions;
use qubit_local_files::LocalTempFileOptions;
use qubit_mime::FileBasedMimeDetector;
use qubit_mime::MimeDetectionPolicy;
use qubit_mime::MimeDetector;
use qubit_mime::MimeDetectorCore;
use qubit_mime::MimeError;
use qubit_mime::MimeResult;

#[derive(Debug)]
struct ContentReadingDetector {
    core: MimeDetectorCore,
}

impl ContentReadingDetector {
    /// Creates a detector that reads the staged file content.
    fn new() -> Self {
        Self {
            core: MimeDetectorCore::default(),
        }
    }
}

impl FileBasedMimeDetector for ContentReadingDetector {
    /// Gets the shared detector core.
    fn core(&self) -> &MimeDetectorCore {
        &self.core
    }

    /// Gets the prefix length used for reader inputs.
    fn max_test_bytes(&self) -> usize {
        16
    }

    /// Returns no filename candidates.
    fn guess_from_filename(&self, _filename: &str) -> Vec<String> {
        Vec::new()
    }

    /// Reads a staged file and returns a MIME type for matching content.
    fn guess_from_local_file(&self, file: &Path) -> MimeResult<Vec<String>> {
        let content = fs::read(file)?;
        if content == b"plain text" {
            Ok(vec!["text/plain".to_owned()])
        } else {
            Ok(Vec::new())
        }
    }
}

#[derive(Debug)]
struct PathRecordingDetector {
    core: MimeDetectorCore,
    seen_path: Mutex<Option<PathBuf>>,
}

impl PathRecordingDetector {
    /// Creates a detector that records the inspected local path.
    fn new() -> Self {
        Self {
            core: MimeDetectorCore::default(),
            seen_path: Mutex::new(None),
        }
    }

    /// Gets the last path inspected by this detector.
    fn seen_path(&self) -> Option<PathBuf> {
        self.seen_path
            .lock()
            .expect("path recorder lock should not be poisoned")
            .clone()
    }
}

impl FileBasedMimeDetector for PathRecordingDetector {
    /// Gets the shared detector core.
    fn core(&self) -> &MimeDetectorCore {
        &self.core
    }

    /// Gets the prefix length used for reader inputs.
    fn max_test_bytes(&self) -> usize {
        16
    }

    /// Returns no filename candidates.
    fn guess_from_filename(&self, _filename: &str) -> Vec<String> {
        Vec::new()
    }

    /// Records the inspected path and returns a fixed content candidate.
    fn guess_from_local_file(&self, file: &Path) -> MimeResult<Vec<String>> {
        *self
            .seen_path
            .lock()
            .expect("path recorder lock should not be poisoned") =
            Some(file.to_path_buf());
        Ok(vec!["application/octet-stream".to_owned()])
    }
}

#[derive(Debug)]
struct FailingDetector {
    core: MimeDetectorCore,
}

impl FailingDetector {
    fn new() -> Self {
        Self {
            core: MimeDetectorCore::default(),
        }
    }
}

impl FileBasedMimeDetector for FailingDetector {
    fn core(&self) -> &MimeDetectorCore {
        &self.core
    }

    fn max_test_bytes(&self) -> usize {
        16
    }

    fn guess_from_filename(&self, _filename: &str) -> Vec<String> {
        Vec::new()
    }

    fn guess_from_local_file(&self, _file: &Path) -> MimeResult<Vec<String>> {
        Err(MimeError::InvalidClassifierInput {
            reason: "forced".to_owned(),
        })
    }
}

/// Verifies byte input is staged before local-file inspection.
#[test]
fn test_detect_by_content_stages_bytes_to_local_file() {
    let detector = ContentReadingDetector::new();

    let detected = detector
        .detect_by_content(b"plain text")
        .expect("content detection should succeed");

    assert_eq!(Some("text/plain".to_owned()), detected);
}

#[test]
fn test_detect_by_content_uses_non_predictable_temporary_file_name() {
    let detector = PathRecordingDetector::new();

    let detected = detector
        .detect_by_content(b"plain text")
        .expect("content detection should succeed");

    assert_eq!(Some("application/octet-stream".to_owned()), detected);
    let staged_path = detector
        .seen_path()
        .expect("staged path should be recorded");
    let filename = staged_path
        .file_name()
        .and_then(|name| name.to_str())
        .expect("staged filename should be UTF-8");
    let predictable_prefix =
        format!("MimeDetectorTemp-{}-", std::process::id());
    assert!(
        filename.starts_with("MimeDetectorTemp-"),
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
fn test_detect_reader_reports_temporary_file_creation_error() {
    const CHILD_ENV: &str = "QUBIT_MIME_CHECK_DETECTOR_TEMPFILE_ERROR";
    const TEST_NAME: &str = "detector::file_based_mime_detector_tests::test_detect_reader_reports_temporary_file_creation_error";

    if std::env::var_os(CHILD_ENV).is_some() {
        let detector = ContentReadingDetector::new();
        let mut reader = std::io::Cursor::new(b"plain text".to_vec());

        let error = detector
            .detect_reader(
                &mut reader,
                None,
                MimeDetectionPolicy::VerifyContent,
            )
            .expect_err(
                "invalid temporary directory should fail reader detection",
            );

        let MimeError::Io(error) = error else {
            panic!("temporary file creation should report an I/O error");
        };
        assert_eq!(std::io::ErrorKind::NotADirectory, error.kind());
        return;
    }

    let temp_dir = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .create_temp_directory_with_options(
            &LocalTempDirectoryOptions::new()
                .with_prefix("qubit-mime-detector-error-"),
        )
        .expect("temporary parent directory should be created");
    let invalid_temp_dir = temp_dir.path().join("not-a-directory");
    fs::write(&invalid_temp_dir, b"not a directory")
        .expect("invalid temporary directory placeholder should be created");
    let output = std::process::Command::new(
        std::env::current_exe()
            .expect("current test binary path should be available"),
    )
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
fn test_detect_reader_creates_missing_temporary_directory() {
    const CHILD_ENV: &str = "QUBIT_MIME_CHECK_DETECTOR_MISSING_TMPDIR";
    const TEST_NAME: &str = "detector::file_based_mime_detector_tests::test_detect_reader_creates_missing_temporary_directory";

    if std::env::var_os(CHILD_ENV).is_some() {
        let detector = ContentReadingDetector::new();
        let mut reader = std::io::Cursor::new(b"plain text".to_vec());

        let detected = detector
            .detect_reader(
                &mut reader,
                None,
                MimeDetectionPolicy::VerifyContent,
            )
            .expect("missing temporary directory should be created");

        assert_eq!(Some("text/plain".to_owned()), detected);
        return;
    }

    let temp_dir = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .create_temp_directory_with_options(
            &LocalTempDirectoryOptions::new()
                .with_prefix("qubit-mime-detector-missing-"),
        )
        .expect("temporary parent directory should be created");
    let missing_temp_dir = temp_dir.path().join("missing").join("nested");
    let output = std::process::Command::new(
        std::env::current_exe()
            .expect("current test binary path should be available"),
    )
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

/// Verifies local-file input delegates directly to the file-based hook.
#[test]
fn test_detect_file_delegates_to_local_file_hook() {
    let temp_file = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .create_temp_file_with_options(&LocalTempFileOptions::new())
        .expect("temporary file should be created");
    let detector = PathRecordingDetector::new();

    let detected = detector
        .detect_file(temp_file.path(), MimeDetectionPolicy::VerifyContent)
        .expect("file detection should succeed");

    assert_eq!(Some("application/octet-stream".to_owned()), detected);
    assert_eq!(Some(temp_file.path().to_path_buf()), detector.seen_path());
}

#[test]
fn test_detect_reader_propagates_file_based_callback_error() {
    let detector = FailingDetector::new();
    let temp_file = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .create_temp_file_with_options(&LocalTempFileOptions::new())
        .expect("temporary file should be created");
    let mut reader = std::io::Cursor::new(b"plain text".to_vec());

    let error = detector
        .detect_reader(&mut reader, None, MimeDetectionPolicy::VerifyContent)
        .expect_err("failing local-file hook should propagate");

    assert!(error.to_string().contains("forced"));
    assert!(
        detector
            .detect_file(temp_file.path(), MimeDetectionPolicy::VerifyContent)
            .expect_err("failing local-file hook should propagate from file")
            .to_string()
            .contains("forced")
    );
}
