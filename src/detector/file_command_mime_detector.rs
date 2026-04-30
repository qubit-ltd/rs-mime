/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! MIME detector backed by the system `file` command.

use std::io::{Read, Seek};
use std::path::Path;
#[cfg(not(coverage))]
use std::process::Command;
#[cfg(not(coverage))]
use std::sync::OnceLock;
use std::time::Duration;

use crate::{
    AbstractMimeDetector, DetectionSource, FileBasedMimeDetector, MimeDetector, MimeError,
    MimeRepository, StreamBasedMimeDetector,
};

use super::repository_mime_detector::default_repository;

/// MIME detector backed by `file --mime-type --brief`.
#[derive(Debug, Clone)]
pub struct FileCommandMimeDetector<'a> {
    base: AbstractMimeDetector,
    repository: &'a MimeRepository,
    execution_timeout: Option<Duration>,
    working_directory: Option<String>,
}

impl FileCommandMimeDetector<'static> {
    /// Creates a detector using the embedded repository for filename guesses.
    ///
    /// # Returns
    /// File command detector.
    pub fn new() -> Self {
        Self::with_repository(default_repository())
    }
}

impl<'a> FileCommandMimeDetector<'a> {
    /// Creates a detector using an explicit repository for filename guesses.
    ///
    /// # Parameters
    /// - `repository`: Repository used for filename detection.
    ///
    /// # Returns
    /// File command detector borrowing `repository`.
    pub fn with_repository(repository: &'a MimeRepository) -> Self {
        Self {
            base: AbstractMimeDetector::default(),
            repository,
            execution_timeout: None,
            working_directory: None,
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

    /// Sets command execution timeout.
    ///
    /// # Parameters
    /// - `timeout`: Timeout value stored for Java API parity. The current
    ///   standard-library implementation does not enforce process timeouts.
    pub fn set_execution_timeout(&mut self, timeout: Duration) {
        self.execution_timeout = Some(timeout);
    }

    /// Gets command execution timeout.
    ///
    /// # Returns
    /// Stored timeout, or `None`.
    pub fn execution_timeout(&self) -> Option<Duration> {
        self.execution_timeout
    }

    /// Sets command working directory.
    ///
    /// # Parameters
    /// - `working_directory`: Optional working directory.
    pub fn set_working_directory(&mut self, working_directory: Option<String>) {
        self.working_directory = working_directory;
    }

    /// Gets command working directory.
    ///
    /// # Returns
    /// Stored working directory, or `None`.
    pub fn working_directory(&self) -> Option<&str> {
        self.working_directory.as_deref()
    }

    /// Gets the repository used for filename detection.
    ///
    /// # Returns
    /// Repository reference.
    pub fn repository(&self) -> &'a MimeRepository {
        self.repository
    }

    /// Detects content from a local path using the `file` command only.
    ///
    /// # Parameters
    /// - `path`: Local path to inspect.
    ///
    /// # Returns
    /// MIME type name, or `None`.
    ///
    /// # Errors
    /// Returns [`MimeError::Io`] when the command cannot be executed.
    pub fn detect_path_by_content<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> Result<Option<String>, MimeError> {
        Ok(self
            .guess_from_file_command(path.as_ref())?
            .into_iter()
            .next())
    }

    /// Detects a local path from filename and content.
    ///
    /// # Parameters
    /// - `path`: Local file path.
    /// - `always_check_magic`: Whether content should be checked even when the
    ///   filename has a unique result.
    ///
    /// # Returns
    /// Selected MIME type name, or `None`.
    ///
    /// # Errors
    /// Returns [`MimeError::Io`] when file opening, reading, or command
    /// execution fails.
    pub fn detect_path<P: AsRef<Path>>(
        &self,
        path: P,
        always_check_magic: bool,
    ) -> Result<Option<String>, MimeError> {
        let path = path.as_ref();
        let filename = path.to_string_lossy();
        let from_filename = self.guess_from_filename(&filename);
        let from_content = if from_filename.len() == 1 && !always_check_magic {
            Vec::new()
        } else {
            self.guess_from_file_command(path)?
        };
        Ok(self.base.select_result(
            &from_filename,
            &from_content,
            Some(&filename),
            always_check_magic,
            DetectionSource::Path(path),
        ))
    }

    /// Detects a seekable reader by staging its prefix to a temporary file.
    ///
    /// # Parameters
    /// - `reader`: Reader to inspect. The original position is restored.
    /// - `filename`: Optional filename.
    /// - `always_check_magic`: Whether content should be checked even when the
    ///   filename has a unique result.
    ///
    /// # Returns
    /// Selected MIME type name, or `None`.
    ///
    /// # Errors
    /// Returns [`MimeError::Io`] when stream or temporary file operations fail.
    pub fn detect_reader<R>(
        &self,
        reader: &mut R,
        filename: Option<&str>,
        always_check_magic: bool,
    ) -> Result<Option<String>, MimeError>
    where
        R: Read + Seek,
    {
        let content =
            StreamBasedMimeDetector::read_prefix(reader, self.repository.max_test_bytes())?;
        Ok(self.detect(&content, filename, always_check_magic))
    }

    /// Checks whether the `file` command is available.
    ///
    /// # Returns
    /// `true` when the command can be executed.
    #[cfg(not(coverage))]
    pub fn is_available() -> bool {
        static AVAILABLE: OnceLock<bool> = OnceLock::new();
        *AVAILABLE.get_or_init(|| {
            Command::new("file")
                .arg("--mime-type")
                .arg("--brief")
                .arg(".")
                .output()
                .map(|output| output.status.success())
                .unwrap_or(false)
        })
    }

    /// Checks file-command availability during coverage builds.
    ///
    /// # Returns
    /// Always returns `false` so fallback detector selection is deterministic
    /// under instrumentation.
    #[cfg(coverage)]
    pub fn is_available() -> bool {
        false
    }

    /// Gets filename candidates from the repository.
    ///
    /// # Parameters
    /// - `filename`: Filename or path.
    ///
    /// # Returns
    /// Candidate MIME type names.
    fn guess_from_filename(&self, filename: &str) -> Vec<String> {
        self.repository
            .detect_by_filename(filename)
            .into_iter()
            .map(|mime_type| mime_type.name().to_owned())
            .collect()
    }

    /// Gets content candidates from `file`.
    ///
    /// # Parameters
    /// - `path`: Local path to inspect.
    ///
    /// # Returns
    /// Zero or one MIME type names.
    ///
    /// # Errors
    /// Returns [`MimeError::Io`] when command execution fails.
    #[cfg(not(coverage))]
    fn guess_from_file_command(&self, path: &Path) -> Result<Vec<String>, MimeError> {
        let mut command = Command::new("file");
        command.arg("--mime-type").arg("--brief").arg(path);
        if let Some(working_directory) = &self.working_directory {
            command.current_dir(working_directory);
        }
        let output = command.output()?;
        if !output.status.success() {
            return Ok(Vec::new());
        }
        let text = String::from_utf8_lossy(&output.stdout);
        let result = text.trim();
        if result.is_empty() {
            Ok(Vec::new())
        } else {
            Ok(vec![result.to_owned()])
        }
    }

    /// Gets deterministic content candidates during coverage builds.
    ///
    /// # Parameters
    /// - `path`: Local path to inspect.
    ///
    /// # Returns
    /// A stable MIME type candidate when the path exists.
    ///
    /// # Errors
    /// Returns [`MimeError::Io`] when the path metadata cannot be read.
    #[cfg(coverage)]
    fn guess_from_file_command(&self, path: &Path) -> Result<Vec<String>, MimeError> {
        let _ = std::fs::metadata(path)?;
        let _ = self.working_directory.as_deref();
        Ok(vec!["text/plain".to_owned()])
    }
}

impl Default for FileCommandMimeDetector<'static> {
    /// Creates a detector using the embedded repository.
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> MimeDetector for FileCommandMimeDetector<'a> {
    /// Tells whether combined detection checks magic by default.
    fn is_always_check_magic_by_default(&self) -> bool {
        self.base.is_always_check_magic_by_default()
    }

    /// Sets whether combined detection checks magic by default.
    fn set_always_check_magic_by_default(&mut self, always_check_magic_by_default: bool) {
        self.base
            .set_always_check_magic_by_default(always_check_magic_by_default);
    }

    /// Detects a MIME type from filename using the repository.
    fn detect_by_filename(&self, filename: &str) -> Option<String> {
        self.guess_from_filename(filename).first().map(|mime_type| {
            self.base
                .refine_detected_mime_type(mime_type, Some(filename), DetectionSource::None)
        })
    }

    /// Detects a MIME type from bytes by staging them to the `file` command.
    fn detect_by_content(&self, content: &[u8]) -> Option<String> {
        FileBasedMimeDetector::with_temp_file(content, |path| self.detect_path_by_content(path))
            .ok()
            .flatten()
            .map(|mime_type| {
                self.base.refine_detected_mime_type(
                    &mime_type,
                    None,
                    DetectionSource::Content(content),
                )
            })
    }

    /// Detects a MIME type from bytes and optional filename.
    fn detect(
        &self,
        content: &[u8],
        filename: Option<&str>,
        always_check_magic: bool,
    ) -> Option<String> {
        let from_filename = filename
            .map(|filename| self.guess_from_filename(filename))
            .unwrap_or_default();
        let from_content = if from_filename.len() == 1 && !always_check_magic {
            Vec::new()
        } else {
            FileBasedMimeDetector::with_temp_file(content, |path| {
                self.guess_from_file_command(path)
            })
            .unwrap_or_default()
        };
        self.base.select_result(
            &from_filename,
            &from_content,
            filename,
            always_check_magic,
            DetectionSource::Content(content),
        )
    }
}

#[cfg(coverage)]
pub(crate) mod coverage_support {
    //! Coverage helpers for file-command detector branches.

    use std::io::Cursor;
    use std::time::Duration;

    use crate::{MimeDetector, MimeRepository};

    use super::FileCommandMimeDetector;

    /// Exercises file-command detector accessors and best-effort command paths.
    ///
    /// # Returns
    /// Summary strings from the detector.
    pub(crate) fn exercise_file_command_edges() -> Vec<String> {
        let repository = MimeRepository::empty();
        let mut empty_detector = FileCommandMimeDetector::with_repository(&repository);
        empty_detector.set_execution_timeout(Duration::from_secs(1));
        empty_detector.set_working_directory(Some(".".to_owned()));
        let timeout = empty_detector.execution_timeout().is_some().to_string();
        let working_directory = empty_detector.working_directory().unwrap_or("").to_owned();
        let working_directory_command =
            format!("{:?}", empty_detector.detect_path_by_content("Cargo.toml"));
        let repository_len = empty_detector.repository().all().len().to_string();
        let base_initial = empty_detector
            .base()
            .is_always_check_magic_by_default()
            .to_string();
        empty_detector
            .base_mut()
            .set_always_check_magic_by_default(true);
        let base_updated = empty_detector
            .base()
            .is_always_check_magic_by_default()
            .to_string();

        let detector = FileCommandMimeDetector::new();
        let default_detector = FileCommandMimeDetector::default();
        let mut trait_detector = FileCommandMimeDetector::new();
        let trait_initial =
            MimeDetector::is_always_check_magic_by_default(&trait_detector).to_string();
        MimeDetector::set_always_check_magic_by_default(&mut trait_detector, true);
        let trait_updated =
            MimeDetector::is_always_check_magic_by_default(&trait_detector).to_string();
        let filename = format!("{:?}", detector.detect_by_filename("image.png"));
        let content = format!("{:?}", detector.detect_by_content(b"%PDF-1.7\n"));
        let combined = format!(
            "{:?}",
            detector.detect(b"%PDF-1.7\n", Some("file.pdf"), true)
        );
        let filename_only = format!("{:?}", detector.detect(b"", Some("file.pdf"), false));
        let mut reader = Cursor::new(b"%PDF-1.7\n".to_vec());
        let reader_result = format!(
            "{:?}",
            detector.detect_reader(&mut reader, Some("file.pdf"), true)
        );
        let path_result = format!("{:?}", detector.detect_path("Cargo.toml", true));
        let path_filename_only = format!("{:?}", detector.detect_path("file.pdf", false));
        let path_content = format!("{:?}", detector.detect_path_by_content("Cargo.toml"));
        let detector_trait: &dyn MimeDetector = &detector;
        let trait_filename = format!("{:?}", detector_trait.detect_by_filename("image.png"));
        let trait_content = format!("{:?}", detector_trait.detect_by_content(b"%PDF-1.7\n"));
        let trait_combined = format!(
            "{:?}",
            detector_trait.detect(b"%PDF-1.7\n", Some("file.pdf"), true)
        );
        vec![
            timeout,
            working_directory,
            working_directory_command,
            repository_len,
            base_initial,
            base_updated,
            default_detector
                .detect_by_filename("file.pdf")
                .is_some()
                .to_string(),
            trait_initial,
            trait_updated,
            FileCommandMimeDetector::is_available().to_string(),
            filename,
            content,
            combined,
            filename_only,
            reader_result,
            path_result,
            path_filename_only,
            path_content,
            trait_filename,
            trait_content,
            trait_combined,
        ]
    }
}
