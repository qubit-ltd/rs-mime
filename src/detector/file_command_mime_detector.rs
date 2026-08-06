// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! MIME detector backed by the system `file` command.
//!
//! This detector uses the embedded repository for filename guesses and invokes
//! `file --mime-type --brief <path>` for content guesses on local files or
//! temporary files staged from seekable readers. It is registered under the
//! built-in provider id `file` and aliases such as `file-command`.

use qubit_command::{
    Command,
    CommandRunner,
};
use qubit_io::std_io::ReadSeek;
use std::path::Path;

use crate::{
    FileBasedMimeDetector,
    MimeConfig,
    MimeDetectionPolicy,
    MimeDetector,
    MimeDetectorCore,
    MimeRepository,
    MimeResult,
};

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
            Self::default_command_runner(&config),
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
        let config = MimeConfig::default();
        Self::with_repository_runner_and_config(
            repository,
            Self::default_command_runner(&config),
            config,
        )
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
        Self::with_repository_runner_and_config(
            repository,
            command_runner,
            MimeConfig::default(),
        )
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
    pub fn with_command_runner(
        mut self,
        command_runner: CommandRunner,
    ) -> Self {
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
    /// Returns [`MimeError::Command`](crate::MimeError::Command) when the
    /// command cannot be executed.
    pub fn detect_file_by_content(
        &self,
        file: &Path,
    ) -> MimeResult<Option<String>> {
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
    /// Returns [`MimeError::Command`](crate::MimeError::Command) when command
    /// execution fails.
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
    /// Returns [`MimeError::Io`](crate::MimeError::Io) when stream operations
    /// fail.
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
    /// Availability is checked by executing `file --mime-type --brief .` with a
    /// quiet command runner. This only validates that the command can be
    /// started successfully in the current process environment; it does not
    /// guarantee that every future file path can be inspected.
    ///
    /// # Returns
    /// `true` when the command can be executed.
    pub fn is_available() -> bool {
        let config = MimeConfig::default();
        Self::default_command_runner(&config)
            .disable_logging(true)
            .run(Self::command_for_path(Path::new(".")))
            .is_ok()
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
    /// Returns [`MimeError::Command`](crate::MimeError::Command) when command
    /// execution fails.
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

    /// Creates the default command runner for file detection.
    ///
    /// Timeout comes from `config.command_timeout()`.
    ///
    /// # Returns
    /// Runner used by the default detector.
    fn default_command_runner(config: &MimeConfig) -> CommandRunner {
        CommandRunner::new(config.command_timeout())
            .bounded_output(config.command_output_max_bytes())
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
            .arg("--")
            .sensitive_arg_os(path)
    }
}

impl Default for FileCommandMimeDetector<'static> {
    /// Creates a detector using the embedded repository.
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> FileBasedMimeDetector for FileCommandMimeDetector<'a> {
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

    /// Guesses MIME type names from a local file using the file command.
    fn guess_from_local_file(&self, file: &Path) -> MimeResult<Vec<String>> {
        self.guess_from_file_command(file)
    }
}
