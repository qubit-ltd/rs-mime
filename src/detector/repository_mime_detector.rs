/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Repository-backed MIME detector.

use std::path::Path;
use std::sync::OnceLock;

use crate::{
    MimeConfig, MimeDetectionPolicy, MimeDetector, MimeDetectorBackend, MimeDetectorCore,
    MimeRepository, MimeResult,
};

const DEFAULT_DATABASE: &str = include_str!("../../resources/freedesktop.org-v2.4.xml");

static DEFAULT_REPOSITORY: OnceLock<MimeRepository> = OnceLock::new();

/// MIME detector backed by a [`MimeRepository`].
#[derive(Debug, Clone)]
pub struct RepositoryMimeDetector<'a> {
    /// The shared detector core.
    core: MimeDetectorCore,
    repository: &'a MimeRepository,
}

impl RepositoryMimeDetector<'static> {
    /// Creates a detector using the embedded freedesktop MIME repository.
    ///
    /// # Returns
    /// A repository-backed detector.
    ///
    /// # Errors
    /// The embedded database is parsed from crate resources and is expected to
    /// be valid; this method keeps a `Result` return type for API consistency.
    pub fn new() -> MimeResult<Self> {
        Ok(Self::with_repository(default_repository()))
    }

    /// Creates a detector using the embedded repository and explicit config.
    ///
    /// # Parameters
    /// - `config`: MIME detector configuration.
    ///
    /// # Returns
    /// A repository-backed detector.
    pub fn from_mime_config(config: MimeConfig) -> Self {
        Self::with_repository_and_config(default_repository(), config)
    }
}

impl Default for RepositoryMimeDetector<'static> {
    fn default() -> Self {
        Self::new().expect("embedded MIME repository should parse")
    }
}

impl<'a> RepositoryMimeDetector<'a> {
    /// Creates a detector using an explicit repository.
    ///
    /// # Parameters
    /// - `repository`: Repository used for all detections.
    ///
    /// # Returns
    /// A detector borrowing `repository`.
    pub fn with_repository(repository: &'a MimeRepository) -> Self {
        Self::with_repository_and_config(repository, MimeConfig::default())
    }

    /// Creates a detector using an explicit repository and config.
    ///
    /// # Parameters
    /// - `repository`: Repository used for all detections.
    /// - `config`: MIME detector configuration.
    ///
    /// # Returns
    /// A detector borrowing `repository`.
    pub fn with_repository_and_config(repository: &'a MimeRepository, config: MimeConfig) -> Self {
        Self {
            core: MimeDetectorCore::from_mime_config(config),
            repository,
        }
    }

    /// Gets the shared detector core.
    ///
    /// # Returns
    /// Shared detector core.
    pub fn core(&self) -> &MimeDetectorCore {
        &self.core
    }

    /// Gets mutable shared detector core.
    ///
    /// # Returns
    /// Mutable shared detector core.
    pub fn core_mut(&mut self) -> &mut MimeDetectorCore {
        &mut self.core
    }

    /// Gets the underlying repository.
    ///
    /// # Returns
    /// Repository used by this detector.
    pub fn repository(&self) -> &'a MimeRepository {
        self.repository
    }

    /// Detects a MIME type from a filename.
    ///
    /// # Parameters
    /// - `filename`: Path or basename to inspect.
    ///
    /// # Returns
    /// First MIME type matched by filename, or `None`.
    pub fn detect_by_filename(&self, filename: &str) -> Option<String> {
        <Self as MimeDetector>::detect_by_filename(self, filename)
    }

    /// Detects a MIME type from content bytes.
    ///
    /// # Parameters
    /// - `bytes`: Content prefix to inspect.
    ///
    /// # Returns
    /// First MIME type matched by magic, or `None`.
    pub fn detect_by_content(&self, bytes: &[u8]) -> Option<String> {
        <Self as MimeDetector>::detect_by_content(self, bytes)
    }

    /// Detects a MIME type from content bytes and an optional filename.
    ///
    /// # Parameters
    /// - `bytes`: Content prefix to inspect.
    /// - `filename`: Optional path or basename used for glob detection.
    /// - `policy`: Strategy for resolving filename and content results.
    ///
    /// # Returns
    /// Selected MIME type name, or `None`.
    pub fn detect_bytes(
        &self,
        bytes: &[u8],
        filename: Option<&str>,
        policy: MimeDetectionPolicy,
    ) -> Option<String> {
        self.detect(bytes, filename, policy)
    }

    /// Detects a MIME type from a seekable reader without consuming its position.
    ///
    /// # Parameters
    /// - `reader`: Reader to inspect. The original stream position is restored.
    /// - `filename`: Optional path or basename used for glob detection.
    /// - `policy`: Strategy for resolving filename and content results.
    ///
    /// # Returns
    /// Selected MIME type name, or `None`.
    ///
    /// # Errors
    /// Returns [`MimeError::Io`](crate::MimeError::Io) when reading or seeking fails.
    pub fn detect_reader(
        &self,
        reader: &mut dyn qubit_io::ReadSeek,
        filename: Option<&str>,
        policy: MimeDetectionPolicy,
    ) -> MimeResult<Option<String>> {
        <Self as MimeDetector>::detect_reader(self, reader, filename, policy)
    }

    /// Detects a MIME type from a local file.
    ///
    /// # Parameters
    /// - `file`: Local file path to open.
    /// - `policy`: Strategy for resolving filename and content results.
    ///
    /// # Returns
    /// Selected MIME type name, or `None`.
    ///
    /// # Errors
    /// Returns [`MimeError::Io`](crate::MimeError::Io) when the file cannot be opened or read.
    pub fn detect_file(
        &self,
        file: &Path,
        policy: MimeDetectionPolicy,
    ) -> MimeResult<Option<String>> {
        <Self as MimeDetector>::detect_file(self, file, policy)
    }

    /// Guesses MIME type names from filename rules.
    ///
    /// # Parameters
    /// - `filename`: Filename or path.
    ///
    /// # Returns
    /// Candidate MIME type names.
    pub fn guess_from_filename(&self, filename: &str) -> Vec<String> {
        self.repository
            .detect_by_filename(filename)
            .into_iter()
            .map(|mime_type| mime_type.name().to_owned())
            .collect()
    }

    /// Guesses MIME type names from content magic rules.
    ///
    /// # Parameters
    /// - `bytes`: Content bytes to inspect.
    ///
    /// # Returns
    /// Candidate MIME type names.
    pub fn guess_from_content(&self, bytes: &[u8]) -> Vec<String> {
        self.repository
            .detect_by_content(bytes)
            .into_iter()
            .map(|mime_type| mime_type.name().to_owned())
            .collect()
    }
}

/// Gets the embedded default repository.
///
/// # Returns
/// Shared parsed repository.
///
pub(crate) fn default_repository() -> &'static MimeRepository {
    DEFAULT_REPOSITORY.get_or_init(|| {
        MimeRepository::from_xml(DEFAULT_DATABASE)
            .expect("embedded freedesktop MIME database should parse")
    })
}

impl<'a> MimeDetectorBackend for RepositoryMimeDetector<'a> {
    /// Gets the shared detector core.
    fn core(&self) -> &MimeDetectorCore {
        &self.core
    }

    /// Gets the maximum content prefix length from the repository.
    fn max_test_bytes(&self) -> usize {
        self.repository.max_test_bytes()
    }

    /// Guesses MIME type names from filename rules.
    fn guess_from_filename(&self, filename: &str) -> Vec<String> {
        RepositoryMimeDetector::guess_from_filename(self, filename)
    }

    /// Guesses MIME type names from content magic rules.
    fn guess_from_content(&self, content: &[u8]) -> MimeResult<Vec<String>> {
        Ok(RepositoryMimeDetector::guess_from_content(self, content))
    }
}

#[cfg(coverage)]
pub(crate) mod coverage_support {
    //! Coverage helpers for detector branches.

    use std::io::{Error, ErrorKind, Read, Result as IoResult, Seek, SeekFrom};

    use crate::{MimeDetectionPolicy, MimeRepository, RepositoryMimeDetector};

    /// Exercises detector accessors and no-match paths.
    ///
    /// # Returns
    /// A summary of observed detector core values.
    pub(crate) fn exercise_detector_edges() -> Vec<String> {
        let repository = MimeRepository::empty();
        let mut detector = RepositoryMimeDetector::with_repository(&repository);
        let core_flag = detector
            .core()
            .media_stream_classifier()
            .is_none()
            .to_string();
        detector.core_mut().set_media_stream_classifier(None);
        let core_mut_flag = detector
            .core()
            .media_stream_classifier()
            .is_none()
            .to_string();
        let repository_len = detector.repository().all().len().to_string();
        let policy_detection = detector
            .detect_bytes(
                b"",
                Some("unknown.bin"),
                MimeDetectionPolicy::PreferFilename,
            )
            .unwrap_or_else(|| "none".to_owned());
        let filename_guesses = detector
            .guess_from_filename("unknown.bin")
            .len()
            .to_string();
        let content_guesses = detector.guess_from_content(b"unknown").len().to_string();
        let path =
            std::env::temp_dir().join(format!("qubit-mime-coverage-{}.pdf", std::process::id()));
        std::fs::write(&path, b"%PDF-1.7\n").expect("coverage temp file should be writable");
        let path_detection = match RepositoryMimeDetector::default()
            .detect_file(&path, MimeDetectionPolicy::PreferFilename)
            .expect("coverage temp file should be readable")
        {
            Some(mime_type) => mime_type,
            None => "none".to_owned(),
        };
        let _ = std::fs::remove_file(&path);
        vec![
            core_flag,
            core_mut_flag,
            repository_len,
            policy_detection,
            filename_guesses,
            content_guesses,
            path_detection,
        ]
    }

    /// Exercises reader error propagation paths.
    ///
    /// # Returns
    /// Error messages from failing reader operations.
    pub(crate) fn exercise_reader_errors() -> Vec<String> {
        let repository = MimeRepository::empty();
        let detector = RepositoryMimeDetector::with_repository(&repository);
        let mut seek_reader = FailingReader::new(FailureMode::Seek);
        let mut read_reader = FailingReader::new(FailureMode::Read);
        let mut buffer = [];
        let seek_read = seek_reader
            .read(&mut buffer)
            .expect("seek-mode reader should allow reads")
            .to_string();
        vec![
            seek_read,
            detector
                .detect_reader(&mut seek_reader, None, MimeDetectionPolicy::VerifyContent)
                .expect_err("seek should fail")
                .to_string(),
            detector
                .detect_reader(&mut read_reader, None, MimeDetectionPolicy::VerifyContent)
                .expect_err("read should fail")
                .to_string(),
        ]
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
        /// Creates a reader failing in a selected operation.
        ///
        /// # Parameters
        /// - `mode`: Operation that should fail.
        ///
        /// # Returns
        /// A failing reader.
        fn new(mode: FailureMode) -> Self {
            Self { mode }
        }
    }

    impl Read for FailingReader {
        /// Reads bytes or fails according to the configured mode.
        fn read(&mut self, _buf: &mut [u8]) -> IoResult<usize> {
            match self.mode {
                FailureMode::Read => Err(Error::new(ErrorKind::Other, "read failed")),
                FailureMode::Seek => Ok(0),
            }
        }
    }

    impl Seek for FailingReader {
        /// Seeks or fails according to the configured mode.
        fn seek(&mut self, _pos: SeekFrom) -> IoResult<u64> {
            match self.mode {
                FailureMode::Seek => Err(Error::new(ErrorKind::Other, "seek failed")),
                FailureMode::Read => Ok(0),
            }
        }
    }
}
