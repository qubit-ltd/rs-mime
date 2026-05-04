/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tempfile::NamedTempFile;

use qubit_mime::{
    FileBasedMimeDetector, MimeDetectionPolicy, MimeDetector, MimeDetectorCore, MimeError,
    MimeResult,
};

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
            .expect("path recorder lock should not be poisoned") = Some(file.to_path_buf());
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

    let detected = detector.detect_by_content(b"plain text");

    assert_eq!(Some("text/plain".to_owned()), detected);
}

/// Verifies local-file input delegates directly to the file-based hook.
#[test]
fn test_detect_file_delegates_to_local_file_hook() {
    let temp_file = NamedTempFile::new().expect("temporary file should be created");
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
    let temp_file = NamedTempFile::new().expect("temporary file should be created");
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
