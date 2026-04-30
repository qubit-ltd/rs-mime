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
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::OnceLock;

use crate::{MimeError, MimeRepository};

const DEFAULT_DATABASE: &str = include_str!("../resources/freedesktop.org-v2.4.xml");

static DEFAULT_REPOSITORY: OnceLock<MimeRepository> = OnceLock::new();

/// MIME detector backed by a [`MimeRepository`].
#[derive(Debug, Clone)]
pub struct RepositoryMimeDetector<'a> {
    repository: &'a MimeRepository,
    always_check_magic_by_default: bool,
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
    pub fn new() -> Result<Self, MimeError> {
        Ok(Self::with_repository(default_repository()))
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
        Self {
            repository,
            always_check_magic_by_default: false,
        }
    }

    /// Gets the underlying repository.
    ///
    /// # Returns
    /// Repository used by this detector.
    pub fn repository(&self) -> &'a MimeRepository {
        self.repository
    }

    /// Tells whether combined detection checks content magic by default.
    ///
    /// # Returns
    /// `true` when default `detect_*` calls check content even for unique
    /// filename matches.
    pub fn is_always_check_magic_by_default(&self) -> bool {
        self.always_check_magic_by_default
    }

    /// Sets whether combined detection checks content magic by default.
    ///
    /// # Parameters
    /// - `always_check_magic_by_default`: New default behavior.
    pub fn set_always_check_magic_by_default(&mut self, always_check_magic_by_default: bool) {
        self.always_check_magic_by_default = always_check_magic_by_default;
    }

    /// Detects a MIME type from a filename.
    ///
    /// # Parameters
    /// - `filename`: Path or basename to inspect.
    ///
    /// # Returns
    /// First MIME type matched by filename, or `None`.
    pub fn detect_by_filename(&self, filename: &str) -> Option<String> {
        self.repository
            .detect_by_filename(filename)
            .first()
            .map(|mime_type| mime_type.name().to_owned())
    }

    /// Detects a MIME type from content bytes.
    ///
    /// # Parameters
    /// - `bytes`: Content prefix to inspect.
    ///
    /// # Returns
    /// First MIME type matched by magic, or `None`.
    pub fn detect_by_content(&self, bytes: &[u8]) -> Option<String> {
        self.repository
            .detect_by_content(bytes)
            .first()
            .map(|mime_type| mime_type.name().to_owned())
    }

    /// Detects a MIME type from content bytes and an optional filename.
    ///
    /// # Parameters
    /// - `bytes`: Content prefix to inspect.
    /// - `filename`: Optional path or basename used for glob detection.
    /// - `always_check_magic`: Whether content magic should be checked even
    ///   when filename detection has a single result.
    ///
    /// # Returns
    /// Selected MIME type name, or `None`.
    pub fn detect_bytes(
        &self,
        bytes: &[u8],
        filename: Option<&str>,
        always_check_magic: bool,
    ) -> Option<String> {
        let filename = filename.unwrap_or("");
        self.repository
            .detect(filename, bytes, always_check_magic)
            .first()
            .map(|mime_type| mime_type.name().to_owned())
    }

    /// Detects a MIME type using the default `always_check_magic` setting.
    ///
    /// # Parameters
    /// - `bytes`: Content prefix to inspect.
    /// - `filename`: Optional path or basename used for glob detection.
    ///
    /// # Returns
    /// Selected MIME type name, or `None`.
    pub fn detect_bytes_default(&self, bytes: &[u8], filename: Option<&str>) -> Option<String> {
        self.detect_bytes(bytes, filename, self.always_check_magic_by_default)
    }

    /// Detects a MIME type from a seekable reader without consuming its position.
    ///
    /// # Parameters
    /// - `reader`: Reader to inspect. The original stream position is restored.
    /// - `filename`: Optional path or basename used for glob detection.
    /// - `always_check_magic`: Whether content magic should be checked even
    ///   when filename detection has a single result.
    ///
    /// # Returns
    /// Selected MIME type name, or `None`.
    ///
    /// # Errors
    /// Returns [`MimeError::Io`] when reading or seeking fails.
    pub fn detect_reader<R>(
        &self,
        reader: &mut R,
        filename: Option<&str>,
        always_check_magic: bool,
    ) -> Result<Option<String>, MimeError>
    where
        R: Read + Seek,
    {
        let position = reader.stream_position()?;
        let mut buffer = vec![0; self.repository.max_test_bytes()];
        let bytes_read = reader.read(&mut buffer)?;
        buffer.truncate(bytes_read);
        reader.seek(SeekFrom::Start(position))?;
        Ok(self.detect_bytes(&buffer, filename, always_check_magic))
    }

    /// Detects a MIME type from a filesystem path.
    ///
    /// # Parameters
    /// - `path`: File path to open.
    /// - `always_check_magic`: Whether content magic should be checked even
    ///   when filename detection has a single result.
    ///
    /// # Returns
    /// Selected MIME type name, or `None`.
    ///
    /// # Errors
    /// Returns [`MimeError::Io`] when the path cannot be opened or read.
    pub fn detect_path<P: AsRef<Path>>(
        &self,
        path: P,
        always_check_magic: bool,
    ) -> Result<Option<String>, MimeError> {
        let path = path.as_ref();
        let filename = path.to_string_lossy();
        let mut file = File::open(path)?;
        self.detect_reader(&mut file, Some(&filename), always_check_magic)
    }
}

/// Gets the embedded default repository.
///
/// # Returns
/// Shared parsed repository.
///
fn default_repository() -> &'static MimeRepository {
    DEFAULT_REPOSITORY.get_or_init(|| {
        MimeRepository::from_xml(DEFAULT_DATABASE)
            .expect("embedded freedesktop MIME database should parse")
    })
}

#[cfg(coverage)]
pub(crate) mod coverage_support {
    //! Coverage helpers for detector branches.

    use std::io::{Error, ErrorKind, Read, Result as IoResult, Seek, SeekFrom};

    use crate::{MimeRepository, RepositoryMimeDetector};

    /// Exercises detector accessors and no-match paths.
    ///
    /// # Returns
    /// A summary of observed detector states.
    pub(crate) fn exercise_detector_edges() -> Vec<String> {
        let repository = MimeRepository::empty();
        let mut detector = RepositoryMimeDetector::with_repository(&repository);
        let initial = detector.is_always_check_magic_by_default().to_string();
        detector.set_always_check_magic_by_default(true);
        let updated = detector.is_always_check_magic_by_default().to_string();
        let repository_len = detector.repository().all().len().to_string();
        let default_detection = detector
            .detect_bytes_default(b"", Some("unknown.bin"))
            .unwrap_or_else(|| "none".to_owned());
        vec![initial, updated, repository_len, default_detection]
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
        vec![
            detector
                .detect_reader(&mut seek_reader, None, true)
                .expect_err("seek should fail")
                .to_string(),
            detector
                .detect_reader(&mut read_reader, None, true)
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
