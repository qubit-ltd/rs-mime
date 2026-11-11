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

use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;

use qubit_command::Command;
use qubit_command::CommandRunner;
use qubit_io::std_io::ReadSeek;

use super::repository_mime_detector::default_repository;
use crate::FileBasedMimeDetector;
use crate::MimeConfig;
use crate::MimeDetectionPolicy;
use crate::MimeDetector;
use crate::MimeDetectorCore;
use crate::MimeError;
use crate::MimeRepository;
use crate::MimeResult;
use crate::command_execution::MimeCommandExecutor;
use crate::command_execution::SystemMimeCommandExecutor;
use crate::command_execution::require_complete_stdout;

/// MIME detector backed by `file --mime-type --brief`.
///
/// # Type Parameters
/// - `'a`: Lifetime of the borrowed MIME repository.
///
/// # Examples
///
/// ```
/// use qubit_mime::{FileCommandMimeDetector, MimeDetector};
/// let detector = FileCommandMimeDetector::new();
/// let _ = detector.detect_by_filename("document.pdf")?;
/// # Ok::<(), qubit_mime::MimeError>(())
/// ```
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
    #[must_use]
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
    #[must_use]
    pub fn from_mime_config(config: MimeConfig) -> Self {
        Self::with_repository_runner_and_config(default_repository(), Self::default_command_runner(&config), config)
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
    #[must_use]
    pub fn with_repository(repository: &'a MimeRepository) -> Self {
        let config = MimeConfig::default();
        Self::with_repository_runner_and_config(repository, Self::default_command_runner(&config), config)
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
    #[must_use]
    pub fn with_repository_and_runner(repository: &'a MimeRepository, command_runner: CommandRunner) -> Self {
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
    #[must_use]
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
    #[must_use]
    #[inline]
    pub fn core(&self) -> &MimeDetectorCore {
        &self.core
    }

    /// Gets mutable shared detector core.
    ///
    /// # Returns
    /// Mutable shared detector core.
    #[must_use]
    #[inline]
    pub fn core_mut(&mut self) -> &mut MimeDetectorCore {
        &mut self.core
    }

    /// Gets the repository used for filename detection.
    ///
    /// # Returns
    /// Repository reference.
    #[must_use]
    #[inline]
    pub fn repository(&self) -> &'a MimeRepository {
        self.repository
    }

    /// Gets the command runner used by this detector.
    ///
    /// # Returns
    /// Runner used for `file` command executions.
    #[inline]
    pub fn command_runner(&self) -> &CommandRunner {
        &self.command_runner
    }

    /// Replaces the command runner used by this detector.
    ///
    /// # Parameters
    /// - `command_runner`: New runner configuration.
    #[inline]
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
    #[must_use]
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
    /// Returns [`MimeError::Command`](crate::MimeError::Command) when the
    /// command cannot be executed.
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
    /// Returns [`MimeError::Command`](crate::MimeError::Command) when command
    /// execution fails.
    pub fn detect_file(&self, file: &Path, policy: MimeDetectionPolicy) -> MimeResult<Option<String>> {
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
        Self::is_available_with_config(&config)
    }

    /// Checks availability using the supplied MIME command budgets.
    ///
    /// # Parameters
    /// - `config`: MIME command budgets used for the probe.
    ///
    /// # Returns
    /// `true` when the `file` availability probe succeeds.
    pub(crate) fn is_available_with_config(config: &MimeConfig) -> bool {
        Self::is_available_with_executor(config, &SystemMimeCommandExecutor)
    }

    /// Runs the availability probe through an injectable command executor.
    ///
    /// # Parameters
    /// - `config`: MIME command budgets used for the probe.
    /// - `executor`: Command executor that runs the availability probe.
    ///
    /// # Returns
    /// `true` when the probe succeeds and its output is complete.
    pub(crate) fn is_available_with_executor(config: &MimeConfig, executor: &dyn MimeCommandExecutor) -> bool {
        executor
            .run(
                &Self::default_command_runner(config).disable_logging(true),
                Self::command_for_path(Path::new(".")),
            )
            .is_ok_and(|output| output.exit_code == Some(0) && require_complete_stdout(&output).is_ok())
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
        self.guess_with_executor(path, &SystemMimeCommandExecutor)
    }

    /// Runs file and validates the actual exit status and complete stdout.
    ///
    /// # Parameters
    /// - `path`: Input path passed after the option separator.
    /// - `executor`: Private execution boundary using this detector's runner.
    ///
    /// # Returns
    /// Zero candidates for empty output, otherwise one trimmed MIME name.
    ///
    /// # Errors
    /// Propagates execution failures; nonzero/signal exits, incomplete or
    /// truncated stdout, and invalid UTF-8 produce DetectorBackend errors.
    pub(crate) fn guess_with_executor(
        &self,
        path: &Path,
        executor: &dyn MimeCommandExecutor,
    ) -> MimeResult<Vec<String>> {
        Self::guess_with_runner_and_executor(&self.command_runner, path, executor)
    }

    /// Runs the file command with an explicit runner and executor.
    ///
    /// # Parameters
    /// - `runner`: Command configuration used for process execution.
    /// - `path`: File passed to the command.
    /// - `executor`: Synchronous command executor.
    ///
    /// # Returns
    /// Candidate MIME type names reported by the file command.
    ///
    /// # Errors
    /// Returns command failures, incomplete output, invalid UTF-8, or an
    /// invalid MIME type response.
    fn guess_with_runner_and_executor(
        runner: &CommandRunner,
        path: &Path,
        executor: &dyn MimeCommandExecutor,
    ) -> MimeResult<Vec<String>> {
        let absolute_path = std::path::absolute(path)?;
        let output = executor.run(runner, Self::command_for_path(&absolute_path))?;
        if output.exit_code != Some(0) {
            return Err(MimeError::detector_backend(
                Self::COMMAND,
                format!("file exited with code {:?}", output.exit_code),
            ));
        }
        let text =
            require_complete_stdout(&output).map_err(|reason| MimeError::detector_backend(Self::COMMAND, reason))?;
        let result = text.trim();
        if result.is_empty() {
            Ok(Vec::new())
        } else if is_mime_name(result) {
            Ok(vec![result.to_owned()])
        } else {
            Err(MimeError::detector_backend(
                Self::COMMAND,
                "file returned an invalid MIME type",
            ))
        }
    }

    /// Stages owned content and runs its inspection on the blocking pool.
    ///
    /// # Parameters
    /// - `content`: Content copied into the worker-owned staging operation.
    /// - `executor`: Executor used by the worker to run the command.
    ///
    /// # Returns
    /// A future containing candidate MIME type names.
    ///
    /// # Errors
    /// Returns allocation, staging, command, output, or MIME validation
    /// errors. Dropping the future after the worker starts does not cancel the
    /// worker or its cleanup.
    pub(crate) fn guess_staged_content_async_with_executor<'b>(
        &'b self,
        content: &'b [u8],
        executor: Arc<dyn MimeCommandExecutor>,
    ) -> Pin<Box<dyn Future<Output = MimeResult<Vec<String>>> + Send + 'b>> {
        Box::pin(async move {
            let mut owned = Vec::new();
            owned
                .try_reserve_exact(content.len())
                .map_err(|source| MimeError::BufferAllocationFailed {
                    requested: content.len(),
                    source,
                })?;
            owned.extend_from_slice(content);
            let runner = self.command_runner.clone();
            blocking::unblock(move || {
                super::file_based_mime_detector::with_temp_file(&owned, |path| {
                    Self::guess_with_runner_and_executor(&runner, path, executor.as_ref())
                })
            })
            .await
        })
    }

    /// Creates the default command runner for file detection.
    ///
    /// Timeout comes from `config.command_timeout()`.
    ///
    /// # Parameters
    /// - `config`: MIME settings that provide command timeout and output
    ///   limits.
    ///
    /// # Returns
    /// Runner used by the default detector.
    fn default_command_runner(config: &MimeConfig) -> CommandRunner {
        CommandRunner::new(config.command_timeout())
            .bounded_output(config.command_output_max_bytes())
            .max_total_output_bytes(config.command_total_output_max_bytes())
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

/// Returns whether `value` is a single MIME type name using ASCII token syntax.
///
/// # Parameters
/// - `value`: The trimmed output produced by the `file` command.
///
/// # Returns
/// `true` if `value` contains exactly one non-empty `type/subtype` token pair.
fn is_mime_name(value: &str) -> bool {
    let Some((kind, subtype)) = value.split_once('/') else {
        return false;
    };
    let token = |part: &str| {
        !part.is_empty()
            && part
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte))
    };
    token(kind) && token(subtype)
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
    ///
    /// # Parameters
    /// - `filename`: File path or basename to match.
    fn guess_from_filename(&self, filename: &str) -> Vec<String> {
        FileCommandMimeDetector::guess_from_filename(self, filename)
    }

    /// Guesses MIME type names from a local file using the file command.
    ///
    /// # Parameters
    /// - `file`: Local file path passed to the `file` command.
    fn guess_from_local_file(&self, file: &Path) -> MimeResult<Vec<String>> {
        self.guess_from_file_command(file)
    }

    /// Runs content staging and command inspection on the blocking pool.
    ///
    /// # Parameters
    /// - `content`: Content bytes to copy and inspect.
    ///
    /// # Returns
    /// A future containing candidate MIME type names.
    ///
    /// # Errors
    /// Propagates allocation, staging, command, and output validation errors.
    fn guess_from_staged_content_async<'b>(
        &'b self,
        content: &'b [u8],
    ) -> Pin<Box<dyn Future<Output = MimeResult<Vec<String>>> + Send + 'b>> {
        self.guess_staged_content_async_with_executor(content, Arc::new(SystemMimeCommandExecutor))
    }
}
