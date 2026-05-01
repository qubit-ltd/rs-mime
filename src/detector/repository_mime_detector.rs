/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Repository-backed MIME detector.

use std::fs::File;
use std::io::{Read, Seek};
use std::path::Path;
use std::sync::OnceLock;

use crate::{
    AbstractMimeDetector, DetectionSource, MimeConfig, MimeDetectionPolicy, MimeDetector,
    MimeRepository, MimeResult, StreamBasedMimeDetector,
};

const DEFAULT_DATABASE: &str = include_str!("../../resources/freedesktop.org-v2.4.xml");

static DEFAULT_REPOSITORY: OnceLock<MimeRepository> = OnceLock::new();

/// MIME detector backed by a [`MimeRepository`].
#[derive(Debug, Clone)]
pub struct RepositoryMimeDetector<'a> {
    base: AbstractMimeDetector,
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
            base: AbstractMimeDetector::from_mime_config(config),
            repository,
        }
    }

    /// Gets the shared detector state.
    ///
    /// # Returns
    /// Shared detector behavior and configuration.
    pub fn base(&self) -> &AbstractMimeDetector {
        &self.base
    }

    /// Gets mutable shared detector state.
    ///
    /// # Returns
    /// Mutable shared detector behavior and configuration.
    pub fn base_mut(&mut self) -> &mut AbstractMimeDetector {
        &mut self.base
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
        self.guess_from_filename(filename).first().map(|mime_type| {
            self.base
                .refine_detected_mime_type(mime_type, Some(filename), DetectionSource::None)
        })
    }

    /// Detects a MIME type from content bytes.
    ///
    /// # Parameters
    /// - `bytes`: Content prefix to inspect.
    ///
    /// # Returns
    /// First MIME type matched by magic, or `None`.
    pub fn detect_by_content(&self, bytes: &[u8]) -> Option<String> {
        self.guess_from_content(bytes).first().map(|mime_type| {
            self.base
                .refine_detected_mime_type(mime_type, None, DetectionSource::Content(bytes))
        })
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
        let from_filename = filename
            .map(|filename| self.guess_from_filename(filename))
            .unwrap_or_default();
        let from_content = if from_filename.len() == 1 && !policy.should_verify_content() {
            Vec::new()
        } else {
            self.guess_from_content(bytes)
        };
        self.base.select_result(
            &from_filename,
            &from_content,
            filename,
            policy,
            DetectionSource::Content(bytes),
        )
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
    pub fn detect_reader<R>(
        &self,
        reader: &mut R,
        filename: Option<&str>,
        policy: MimeDetectionPolicy,
    ) -> MimeResult<Option<String>>
    where
        R: Read + Seek,
    {
        let buffer =
            StreamBasedMimeDetector::read_prefix(reader, self.repository.max_test_bytes())?;
        Ok(self.detect_bytes(&buffer, filename, policy))
    }

    /// Detects a MIME type from a filesystem path.
    ///
    /// # Parameters
    /// - `path`: File path to open.
    /// - `policy`: Strategy for resolving filename and content results.
    ///
    /// # Returns
    /// Selected MIME type name, or `None`.
    ///
    /// # Errors
    /// Returns [`MimeError::Io`](crate::MimeError::Io) when the path cannot be opened or read.
    pub fn detect_path<P: AsRef<Path>>(
        &self,
        path: P,
        policy: MimeDetectionPolicy,
    ) -> MimeResult<Option<String>> {
        let path = path.as_ref();
        let filename = path.to_string_lossy();
        let mut file = File::open(path)?;
        let buffer =
            StreamBasedMimeDetector::read_prefix(&mut file, self.repository.max_test_bytes())?;
        let from_filename = self.guess_from_filename(&filename);
        let from_content = if from_filename.len() == 1 && !policy.should_verify_content() {
            Vec::new()
        } else {
            self.guess_from_content(&buffer)
        };
        Ok(self.base.select_result(
            &from_filename,
            &from_content,
            Some(&filename),
            policy,
            DetectionSource::Path(path),
        ))
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

impl<'a> MimeDetector for RepositoryMimeDetector<'a> {
    /// Detects a MIME type from filename.
    fn detect_by_filename(&self, filename: &str) -> Option<String> {
        RepositoryMimeDetector::detect_by_filename(self, filename)
    }

    /// Detects a MIME type from content bytes.
    fn detect_by_content(&self, content: &[u8]) -> Option<String> {
        RepositoryMimeDetector::detect_by_content(self, content)
    }

    /// Detects a MIME type from content bytes and optional filename.
    fn detect(
        &self,
        content: &[u8],
        filename: Option<&str>,
        policy: MimeDetectionPolicy,
    ) -> Option<String> {
        RepositoryMimeDetector::detect_bytes(self, content, filename, policy)
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
    /// A summary of observed detector states.
    pub(crate) fn exercise_detector_edges() -> Vec<String> {
        let repository = MimeRepository::empty();
        let mut detector = RepositoryMimeDetector::with_repository(&repository);
        let base_flag = detector
            .base()
            .media_stream_classifier()
            .is_none()
            .to_string();
        detector.base_mut().set_media_stream_classifier(None);
        let base_mut_flag = detector
            .base()
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
            .detect_path(&path, MimeDetectionPolicy::PreferFilename)
            .expect("coverage temp file should be readable")
        {
            Some(mime_type) => mime_type,
            None => "none".to_owned(),
        };
        let _ = std::fs::remove_file(&path);
        vec![
            base_flag,
            base_mut_flag,
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
