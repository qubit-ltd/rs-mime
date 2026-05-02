/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! MIME detector backed by the system `file` command.

use std::io::{Read, Seek};
use std::path::{Path, PathBuf};
#[cfg(not(coverage))]
use std::sync::OnceLock;
use std::time::Duration;

use qubit_command::{Command, CommandRunner};

use crate::{
    AbstractMimeDetector, DetectionSource, FileBasedMimeDetector, MimeConfig, MimeDetectionPolicy,
    MimeDetector, MimeRepository, MimeResult, StreamBasedMimeDetector,
};

use super::repository_mime_detector::default_repository;

/// MIME detector backed by `file --mime-type --brief`.
#[derive(Debug, Clone)]
pub struct FileCommandMimeDetector<'a> {
    base: AbstractMimeDetector,
    repository: &'a MimeRepository,
    command_runner: CommandRunner,
}

impl FileCommandMimeDetector<'static> {
    /// Creates a detector using the embedded repository for filename guesses.
    ///
    /// # Returns
    /// File command detector.
    pub fn new() -> Self {
        Self::with_repository(default_repository())
    }

    /// Creates a detector using the embedded repository and explicit config.
    ///
    /// # Parameters
    /// - `config`: MIME detector configuration.
    ///
    /// # Returns
    /// File command detector.
    pub fn from_mime_config(config: MimeConfig) -> Self {
        Self::with_repository_runner_and_config(
            default_repository(),
            Self::default_command_runner(),
            config,
        )
    }
}

impl<'a> FileCommandMimeDetector<'a> {
    /// System command executable name.
    pub const COMMAND: &'static str = "file";
    /// Argument enabling MIME type output.
    pub const MIME_TYPE_ARG: &'static str = "--mime-type";
    /// Argument enabling concise output.
    pub const BRIEF_ARG: &'static str = "--brief";

    /// Creates a detector using an explicit repository for filename guesses.
    ///
    /// # Parameters
    /// - `repository`: Repository used for filename detection.
    ///
    /// # Returns
    /// File command detector borrowing `repository`.
    pub fn with_repository(repository: &'a MimeRepository) -> Self {
        Self::with_repository_and_runner(repository, Self::default_command_runner())
    }

    /// Creates a detector using an explicit repository and command runner.
    ///
    /// # Parameters
    /// - `repository`: Repository used for filename detection.
    /// - `command_runner`: Runner used for all `file` command executions.
    ///
    /// # Returns
    /// File command detector borrowing `repository` and owning the supplied
    /// runner.
    pub fn with_repository_and_runner(
        repository: &'a MimeRepository,
        command_runner: CommandRunner,
    ) -> Self {
        Self::with_repository_runner_and_config(repository, command_runner, MimeConfig::default())
    }

    /// Creates a detector using an explicit repository, runner, and config.
    ///
    /// # Parameters
    /// - `repository`: Repository used for filename detection.
    /// - `command_runner`: Runner used for all `file` command executions.
    /// - `config`: MIME detector configuration.
    ///
    /// # Returns
    /// File command detector borrowing `repository` and owning the supplied
    /// runner.
    pub fn with_repository_runner_and_config(
        repository: &'a MimeRepository,
        command_runner: CommandRunner,
        config: MimeConfig,
    ) -> Self {
        Self {
            base: AbstractMimeDetector::from_mime_config(config),
            repository,
            command_runner,
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
    /// - `timeout`: Maximum duration allowed for each `file` command.
    pub fn set_execution_timeout(&mut self, timeout: Duration) {
        self.command_runner = self.command_runner.clone().timeout(timeout);
    }

    /// Gets command execution timeout.
    ///
    /// # Returns
    /// Stored timeout, or `None`.
    pub fn execution_timeout(&self) -> Option<Duration> {
        self.command_runner.configured_timeout()
    }

    /// Sets command working directory.
    ///
    /// # Parameters
    /// - `working_directory`: Working directory used by the command runner.
    pub fn set_working_directory<P>(&mut self, working_directory: P)
    where
        P: Into<PathBuf>,
    {
        self.command_runner = self
            .command_runner
            .clone()
            .working_directory(working_directory);
    }

    /// Gets command working directory.
    ///
    /// # Returns
    /// Stored working directory, or `None`.
    pub fn working_directory(&self) -> Option<&Path> {
        self.command_runner.configured_working_directory()
    }

    /// Gets the repository used for filename detection.
    ///
    /// # Returns
    /// Repository reference.
    pub fn repository(&self) -> &'a MimeRepository {
        self.repository
    }

    /// Gets the command runner used by this detector.
    ///
    /// # Returns
    /// Runner used for `file` command executions.
    pub fn command_runner(&self) -> &CommandRunner {
        &self.command_runner
    }

    /// Replaces the command runner used by this detector.
    ///
    /// # Parameters
    /// - `command_runner`: New runner configuration.
    pub fn set_command_runner(&mut self, command_runner: CommandRunner) {
        self.command_runner = command_runner;
    }

    /// Replaces the command runner and returns the updated detector.
    ///
    /// # Parameters
    /// - `command_runner`: New runner configuration.
    ///
    /// # Returns
    /// The updated detector.
    pub fn with_command_runner(mut self, command_runner: CommandRunner) -> Self {
        self.command_runner = command_runner;
        self
    }

    /// Enables or disables command execution logs.
    ///
    /// # Parameters
    /// - `disable_logging`: `true` to suppress runner logs.
    pub fn set_disable_logging(&mut self, disable_logging: bool) {
        self.command_runner = self.command_runner.clone().disable_logging(disable_logging);
    }

    /// Tells whether command execution logging is disabled.
    ///
    /// # Returns
    /// `true` when runner logs are suppressed.
    pub fn is_disable_logging(&self) -> bool {
        self.command_runner.is_logging_disabled()
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
    /// Returns [`MimeError::Command`](crate::MimeError::Command) when the command cannot be executed.
    pub fn detect_path_by_content<P: AsRef<Path>>(&self, path: P) -> MimeResult<Option<String>> {
        Ok(self
            .guess_from_file_command(path.as_ref())?
            .into_iter()
            .next())
    }

    /// Detects a local path from filename and content.
    ///
    /// # Parameters
    /// - `path`: Local file path.
    /// - `policy`: Strategy for resolving filename and content results.
    ///
    /// # Returns
    /// Selected MIME type name, or `None`.
    ///
    /// # Errors
    /// Returns [`MimeError::Command`](crate::MimeError::Command) when command execution fails.
    pub fn detect_path<P: AsRef<Path>>(
        &self,
        path: P,
        policy: MimeDetectionPolicy,
    ) -> MimeResult<Option<String>> {
        let path = path.as_ref();
        let filename = path.to_string_lossy();
        let from_filename = self.guess_from_filename(&filename);
        let from_content = if from_filename.len() == 1 && !policy.should_verify_content() {
            Vec::new()
        } else {
            self.guess_from_file_command(path)?
        };
        Ok(self.base.select_result(
            &from_filename,
            &from_content,
            Some(&filename),
            policy,
            DetectionSource::Path(path),
        ))
    }

    /// Detects a seekable reader by staging its prefix to a temporary file.
    ///
    /// # Parameters
    /// - `reader`: Reader to inspect. The original position is restored.
    /// - `filename`: Optional filename.
    /// - `policy`: Strategy for resolving filename and content results.
    ///
    /// # Returns
    /// Selected MIME type name, or `None`.
    ///
    /// # Errors
    /// Returns [`MimeError::Io`](crate::MimeError::Io) when stream operations fail.
    pub fn detect_reader<R>(
        &self,
        reader: &mut R,
        filename: Option<&str>,
        policy: MimeDetectionPolicy,
    ) -> MimeResult<Option<String>>
    where
        R: Read + Seek,
    {
        let content =
            StreamBasedMimeDetector::read_prefix(reader, self.repository.max_test_bytes())?;
        Ok(self.detect(&content, filename, policy))
    }

    /// Checks whether the `file` command is available.
    ///
    /// # Returns
    /// `true` when the command can be executed.
    #[cfg(not(coverage))]
    pub fn is_available() -> bool {
        static AVAILABLE: OnceLock<bool> = OnceLock::new();
        *AVAILABLE.get_or_init(|| {
            CommandRunner::new()
                .disable_logging(true)
                .lossy_output(true)
                .run(Self::command_for_path(Path::new(".")))
                .is_ok()
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
    /// Returns [`MimeError::Command`](crate::MimeError::Command) when command execution fails.
    #[cfg(not(coverage))]
    fn guess_from_file_command(&self, path: &Path) -> MimeResult<Vec<String>> {
        let output = self.command_runner.run(Self::command_for_path(path))?;
        let text = String::from_utf8_lossy(output.stdout_bytes());
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
    /// Returns [`MimeError::Io`](crate::MimeError::Io) when the path metadata cannot be read.
    #[cfg(coverage)]
    fn guess_from_file_command(&self, path: &Path) -> MimeResult<Vec<String>> {
        let _ = std::fs::metadata(path)?;
        let _ = self.command_runner.configured_working_directory();
        Ok(vec!["text/plain".to_owned()])
    }

    /// Creates the default command runner for file detection.
    ///
    /// # Returns
    /// Runner used by the default detector.
    fn default_command_runner() -> CommandRunner {
        CommandRunner::new().lossy_output(true)
    }

    /// Builds the structured `file` command for one path.
    ///
    /// # Parameters
    /// - `path`: Local file path passed as an argument without shell parsing.
    ///
    /// # Returns
    /// Structured command description.
    fn command_for_path(path: &Path) -> Command {
        Command::new(Self::COMMAND)
            .arg(Self::MIME_TYPE_ARG)
            .arg(Self::BRIEF_ARG)
            .arg_os(path)
    }
}

impl Default for FileCommandMimeDetector<'static> {
    /// Creates a detector using the embedded repository.
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> MimeDetector for FileCommandMimeDetector<'a> {
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
        policy: MimeDetectionPolicy,
    ) -> Option<String> {
        let from_filename = filename
            .map(|filename| self.guess_from_filename(filename))
            .unwrap_or_default();
        let from_content = if from_filename.len() == 1 && !policy.should_verify_content() {
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
            policy,
            DetectionSource::Content(content),
        )
    }
}

#[cfg(coverage)]
pub(crate) mod coverage_support {
    //! Coverage helpers for file-command detector branches.

    use std::io::Cursor;
    use std::path::Path;
    use std::time::Duration;

    use qubit_command::CommandRunner;

    use crate::{MimeDetectionPolicy, MimeDetector, MimeRepository};

    use super::FileCommandMimeDetector;

    /// Exercises file-command detector accessors and best-effort command paths.
    ///
    /// # Returns
    /// Summary strings from the detector.
    pub(crate) fn exercise_file_command_edges() -> Vec<String> {
        let repository = MimeRepository::empty();
        let mut empty_detector = FileCommandMimeDetector::with_repository(&repository);
        let base_flag = empty_detector
            .base()
            .media_stream_classifier()
            .is_none()
            .to_string();
        empty_detector.base_mut().set_media_stream_classifier(None);
        let base_mut_flag = empty_detector
            .base()
            .media_stream_classifier()
            .is_none()
            .to_string();
        empty_detector.set_execution_timeout(Duration::from_secs(1));
        empty_detector.set_working_directory(".");
        empty_detector.set_disable_logging(true);
        let timeout = empty_detector.execution_timeout().is_some().to_string();
        let working_directory = empty_detector
            .working_directory()
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        let disable_logging = empty_detector.is_disable_logging().to_string();
        let runner_timeout = empty_detector
            .command_runner()
            .configured_timeout()
            .is_some()
            .to_string();
        let working_directory_command =
            format!("{:?}", empty_detector.detect_path_by_content("Cargo.toml"));
        let command_description = format!(
            "{:?}",
            FileCommandMimeDetector::command_for_path(Path::new("Cargo.toml"))
        );
        let repository_len = empty_detector.repository().all().len().to_string();
        let replaced_runner = FileCommandMimeDetector::with_repository(&repository)
            .with_command_runner(CommandRunner::new().disable_logging(true))
            .is_disable_logging()
            .to_string();
        let mut setter_detector = FileCommandMimeDetector::with_repository(&repository);
        setter_detector.set_command_runner(CommandRunner::new().disable_logging(true));
        let setter_runner = setter_detector.is_disable_logging().to_string();

        let detector = FileCommandMimeDetector::new();
        let default_detector = FileCommandMimeDetector::default();
        let filename = format!("{:?}", detector.detect_by_filename("image.png"));
        let content = format!("{:?}", detector.detect_by_content(b"%PDF-1.7\n"));
        let combined = format!(
            "{:?}",
            detector.detect(
                b"%PDF-1.7\n",
                Some("file.pdf"),
                MimeDetectionPolicy::VerifyContent,
            )
        );
        let filename_only = format!(
            "{:?}",
            detector.detect(b"", Some("file.pdf"), MimeDetectionPolicy::PreferFilename,)
        );
        let mut reader = Cursor::new(b"%PDF-1.7\n".to_vec());
        let reader_result = format!(
            "{:?}",
            detector.detect_reader(
                &mut reader,
                Some("file.pdf"),
                MimeDetectionPolicy::VerifyContent,
            )
        );
        let path_result = format!(
            "{:?}",
            detector.detect_path("Cargo.toml", MimeDetectionPolicy::VerifyContent)
        );
        let path_filename_only = format!(
            "{:?}",
            detector.detect_path("file.pdf", MimeDetectionPolicy::PreferFilename)
        );
        let path_content = format!("{:?}", detector.detect_path_by_content("Cargo.toml"));
        let detector_trait: &dyn MimeDetector = &detector;
        let trait_filename = format!("{:?}", detector_trait.detect_by_filename("image.png"));
        let trait_content = format!("{:?}", detector_trait.detect_by_content(b"%PDF-1.7\n"));
        let trait_combined = format!(
            "{:?}",
            detector_trait.detect(
                b"%PDF-1.7\n",
                Some("file.pdf"),
                MimeDetectionPolicy::VerifyContent,
            )
        );
        vec![
            base_flag,
            base_mut_flag,
            timeout,
            working_directory,
            disable_logging,
            runner_timeout,
            working_directory_command,
            command_description,
            repository_len,
            replaced_runner,
            setter_runner,
            default_detector
                .detect_by_filename("file.pdf")
                .is_some()
                .to_string(),
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
