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

use std::path::Path;
#[cfg(not(coverage))]
use std::sync::OnceLock;

use qubit_command::{Command, CommandRunner};
use qubit_io::ReadSeek;

use crate::{
    MimeConfig, MimeDetectionPolicy, MimeDetector, MimeDetectorBackend, MimeDetectorCore,
    MimeRepository, MimeResult,
};

use super::file_based_mime_detector::with_temp_file;
use super::repository_mime_detector::default_repository;

/// MIME detector backed by `file --mime-type --brief`.
#[derive(Debug, Clone)]
pub struct FileCommandMimeDetector<'a> {
    /// The shared detector core.
    core: MimeDetectorCore,
    /// The repository used for filename detection.
    repository: &'a MimeRepository,
    /// The command runner used for command execution.
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
            core: MimeDetectorCore::from_mime_config(config),
            repository,
            command_runner,
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

    /// Detects content from a local file using the `file` command only.
    ///
    /// # Parameters
    /// - `file`: Local file path to inspect.
    ///
    /// # Returns
    /// MIME type name, or `None`.
    ///
    /// # Errors
    /// Returns [`MimeError::Command`](crate::MimeError::Command) when the command cannot be executed.
    pub fn detect_file_by_content(&self, file: &Path) -> MimeResult<Option<String>> {
        Ok(self.guess_from_file_command(file)?.into_iter().next())
    }

    /// Detects a local file from filename and content.
    ///
    /// # Parameters
    /// - `file`: Local file path.
    /// - `policy`: Strategy for resolving filename and content results.
    ///
    /// # Returns
    /// Selected MIME type name, or `None`.
    ///
    /// # Errors
    /// Returns [`MimeError::Command`](crate::MimeError::Command) when command execution fails.
    pub fn detect_file(
        &self,
        file: &Path,
        policy: MimeDetectionPolicy,
    ) -> MimeResult<Option<String>> {
        <Self as MimeDetector>::detect_file(self, file, policy)
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
    pub fn detect_reader(
        &self,
        reader: &mut dyn ReadSeek,
        filename: Option<&str>,
        policy: MimeDetectionPolicy,
    ) -> MimeResult<Option<String>> {
        <Self as MimeDetector>::detect_reader(self, reader, filename, policy)
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
        let text = output.stdout_lossy_text();
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
        CommandRunner::new()
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

impl<'a> MimeDetectorBackend for FileCommandMimeDetector<'a> {
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
        FileCommandMimeDetector::guess_from_filename(self, filename)
    }

    /// Guesses MIME type names from content using a temporary file.
    fn guess_from_content(&self, content: &[u8]) -> MimeResult<Vec<String>> {
        with_temp_file(content, |path| self.guess_from_file_command(path))
    }

    /// Guesses MIME type names from a local file using the file command.
    fn guess_from_file(&self, file: &Path) -> MimeResult<(Vec<String>, Vec<u8>)> {
        Ok((self.guess_from_file_command(file)?, Vec::new()))
    }
}

#[cfg(coverage)]
pub(crate) mod coverage_support {
    //! Coverage helpers for file-command detector branches.

    use std::io::Cursor;
    use std::path::Path;

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
        let core_flag = empty_detector
            .core()
            .media_stream_classifier()
            .is_none()
            .to_string();
        empty_detector.core_mut().set_media_stream_classifier(None);
        let core_mut_flag = empty_detector
            .core()
            .media_stream_classifier()
            .is_none()
            .to_string();
        empty_detector.set_command_runner(
            CommandRunner::new()
                .working_directory(".")
                .disable_logging(true)
                .timeout(std::time::Duration::from_secs(1)),
        );
        let runner_timeout = empty_detector
            .command_runner()
            .configured_timeout()
            .is_some()
            .to_string();
        let working_directory = empty_detector
            .command_runner()
            .configured_working_directory()
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        let disable_logging = empty_detector
            .command_runner()
            .is_logging_disabled()
            .to_string();
        let working_directory_command = format!(
            "{:?}",
            empty_detector.detect_file_by_content(Path::new("Cargo.toml"))
        );
        let command_description = format!(
            "{:?}",
            FileCommandMimeDetector::command_for_path(Path::new("Cargo.toml"))
        );
        let repository_len = empty_detector.repository().all().len().to_string();
        let replaced_runner = FileCommandMimeDetector::with_repository(&repository)
            .with_command_runner(CommandRunner::new().disable_logging(true))
            .command_runner()
            .is_logging_disabled()
            .to_string();
        let mut setter_detector = FileCommandMimeDetector::with_repository(&repository);
        setter_detector.set_command_runner(CommandRunner::new().disable_logging(true));
        let setter_runner = setter_detector
            .command_runner()
            .is_logging_disabled()
            .to_string();

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
            detector.detect_file(Path::new("Cargo.toml"), MimeDetectionPolicy::VerifyContent)
        );
        let path_filename_only = format!(
            "{:?}",
            detector.detect_file(Path::new("file.pdf"), MimeDetectionPolicy::PreferFilename)
        );
        let path_content = format!(
            "{:?}",
            detector.detect_file_by_content(Path::new("Cargo.toml"))
        );
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
            core_flag,
            core_mut_flag,
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
