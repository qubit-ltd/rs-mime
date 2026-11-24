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
//!
//! Local-path detection passes the path to the system `file` command without
//! `-L`. The default runner therefore preserves the installed command's
//! path-object semantics, which may vary by platform, version, and environment.
//! An injected executor determines its own behavior. Applications needing a
//! consistent link policy should apply it while opening an ordinary file and
//! use reader detection; a separate metadata check does not eliminate races.

use std::path::Path;
use std::sync::Arc;

use qubit_command::Command;
use qubit_command::CommandRunner;
use qubit_io::std_io::ReadSeek;
use qubit_operation::OperationScope;
use qubit_operation::StopReport;

use super::repository_mime_detector::default_repository;
use crate::FileBasedMimeDetector;
use crate::MimeConfig;
use crate::MimeDetectionPolicy;
use crate::MimeDetector;
use crate::MimeDetectorCore;
use crate::MimeError;
use crate::MimeRepository;
use crate::MimeResult;
use crate::command_execution::AsyncMimeCommandExecutor;
use crate::command_execution::ManagedCommandExecutor;
use crate::command_execution::ManagedMimeCommand;
use crate::command_execution::MimeCommandExecutor;
use crate::command_execution::MimeExecutionWorker;
use crate::command_execution::SystemMimeCommandExecutor;
use crate::command_execution::require_complete_stdout;
use crate::detector::MimeExecutionServices;
use crate::detector::async_mime_detector_backend::AsyncMimeDetectorBackend;
use crate::detector::async_mime_detector_backend::MimeFuture;
use crate::detector::async_mime_detector_backend::RandomAccessCapability;
use crate::detector::mime_detection_report::MimeCandidate;
use crate::detector::mime_detection_report::MimeCandidates;
use crate::detector::mime_input::BorrowedMimeInput;
use crate::detector::mime_input::InputExtent;
use crate::detector::mime_input::InspectedExtent;
use crate::detector::mime_input::OwnedMimeInput;
use crate::detector::mime_operation_options::MimeOperationOptions;
use crate::managed_temp_staging::ManagedStagingLimits;
use crate::managed_temp_staging::stage_borrowed_bytes_in;
use crate::managed_temp_staging::stage_owned_in;

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
#[derive(Clone)]
pub struct FileCommandMimeDetector<'a> {
    /// The shared detector core.
    core: MimeDetectorCore,
    /// The repository used for filename detection.
    repository: &'a MimeRepository,
    /// The command runner used for command execution.
    command_runner: CommandRunner,
    /// Prepared managed operation services for asynchronous detection.
    execution_services: Option<MimeExecutionServices>,
    /// Maximum bytes that may be staged for command input.
    max_staging_bytes: u64,
    #[cfg(test)]
    test_command_executor: Option<Arc<dyn AsyncMimeCommandExecutor>>,
    #[cfg(test)]
    test_runner_invocations: Option<Arc<std::sync::atomic::AtomicUsize>>,
}

impl std::fmt::Debug for FileCommandMimeDetector<'_> {
    /// Formats detector configuration without exposing command internals.
    ///
    /// # Parameters
    /// - `formatter`: Receives the formatted detector fields.
    ///
    /// # Returns
    /// `Ok(())` after writing the detector's debug fields.
    ///
    /// # Errors
    /// Returns the formatter error if one of the fields cannot be written.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FileCommandMimeDetector")
            .field("repository", &self.repository)
            .field("command_runner", &self.command_runner)
            .field("execution_services_configured", &self.execution_services.is_some())
            .finish_non_exhaustive()
    }
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
        let max_staging_bytes = config.media_stream_max_staging_size();
        let mut core = MimeDetectorCore::from_mime_config(config);
        core.set_mime_relations(repository.relations());
        Self {
            core,
            repository,
            command_runner,
            execution_services: None,
            max_staging_bytes,
            #[cfg(test)]
            test_command_executor: None,
            #[cfg(test)]
            test_runner_invocations: None,
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
    #[must_use = "use the returned command runner to inspect or clone its configuration"]
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

    /// Attaches prepared MIME operation services for explicit async detection.
    ///
    /// # Parameters
    /// - `services`: Services used to submit and account for async work.
    ///
    /// # Returns
    /// The detector configured with those services.
    #[must_use]
    pub fn with_execution_services(mut self, services: MimeExecutionServices) -> Self {
        self.execution_services = Some(services);
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
    /// execution fails. The default runner passes the path to the system `file`
    /// command without `-L`; the command's treatment of links, dangling links,
    /// and directories depends on platform, version, and environment. A custom
    /// executor defines its own behavior.
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

    #[cfg(test)]
    fn with_test_command_executor(mut self, executor: Arc<dyn AsyncMimeCommandExecutor>) -> Self {
        self.test_command_executor = Some(executor);
        self
    }

    #[cfg(test)]
    fn with_test_runner_invocations(mut self, runner_invocations: Arc<std::sync::atomic::AtomicUsize>) -> Self {
        self.test_runner_invocations = Some(runner_invocations);
        self
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

    /// Gets filename candidates from an already extracted native basename.
    ///
    /// # Parameters
    /// - `basename`: Native basename whose filename rules are matched.
    ///
    /// # Returns
    /// Candidate MIME type names in repository order.
    fn guess_from_filename_basename(&self, basename: &str) -> Vec<String> {
        self.repository
            .detect_by_filename_basename(basename)
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

impl AsyncMimeDetectorBackend for FileCommandMimeDetector<'_> {
    /// Requires the complete resource before the system command can inspect it.
    ///
    /// # Returns
    /// [`ContentRequirement::Complete`](crate::ContentRequirement::Complete).
    fn content_requirement(&self) -> crate::ContentRequirement {
        crate::ContentRequirement::Complete
    }

    /// Declares that this backend does not sample random ranges.
    ///
    /// # Returns
    /// [`RandomAccessCapability::Unsupported`].
    fn random_access_capability(&self) -> RandomAccessCapability {
        RandomAccessCapability::Unsupported
    }

    /// Runs the system detector within the supplied managed operation scope.
    ///
    /// # Parameters
    /// - `scope`: Operation context controlling execution and cancellation.
    /// - `input`: Complete MIME input to stage and inspect.
    /// - `options`: Per-operation resource and output limits.
    ///
    /// # Returns
    /// A future resolving to the candidates and input evidence.
    ///
    /// # Errors
    /// The future resolves to a MIME error if the input is incomplete,
    /// operation services are unavailable, staging exceeds its budget or
    /// fails, command execution fails, or command output is incomplete or
    /// invalid.
    fn guess_content_in<'b>(
        &'b self,
        scope: &'b OperationScope,
        input: BorrowedMimeInput<'b>,
        options: &'b MimeOperationOptions,
    ) -> MimeFuture<'b, MimeCandidates> {
        Box::pin(async move {
            if let Some(reason) = scope.context().stop_reason() {
                return Err(MimeError::OperationStopped(Box::new(StopReport {
                    reason,
                    accepted_at: None,
                })));
            }
            if matches!(
                input,
                BorrowedMimeInput::Bytes {
                    extent: InputExtent::Prefix,
                    ..
                }
            ) {
                return Err(MimeError::CompleteContentRequired);
            }
            let services = self
                .execution_services
                .as_ref()
                .ok_or(MimeError::ExecutionServicesRequired)?;
            if options.max_staging_bytes == Some(0) {
                return Err(MimeError::InvalidOperationBudget);
            }
            let max_bytes = options
                .max_staging_bytes
                .unwrap_or(self.max_staging_bytes)
                .min(self.max_staging_bytes)
                .min(services.staging_budget().limit());
            if max_bytes == 0 {
                return Err(MimeError::InvalidOperationBudget);
            }
            let worker = MimeExecutionWorker::new(Arc::clone(services.execution()));
            let limits = ManagedStagingLimits {
                max_bytes,
                budget: services.staging_budget(),
                worker: worker.clone(),
            };
            // Byte staging does not consult or charge provider-read context.
            let staged = match input {
                BorrowedMimeInput::Bytes { content, extent, .. } => {
                    stage_borrowed_bytes_in(scope, content, extent, limits).await?
                }
                BorrowedMimeInput::Provider { file_system, path } => {
                    let read_tree = services.read_tree_for_operation(scope.id())?;
                    stage_owned_in(
                        scope,
                        OwnedMimeInput::Provider {
                            file_system: Arc::new(file_system.clone()),
                            path: path.clone(),
                        },
                        limits,
                        read_tree,
                    )
                    .await?
                }
                BorrowedMimeInput::ManagedProvider { .. } => {
                    return Err(MimeError::InvalidClassifierInput {
                        reason: "managed provider sessions are only accepted by the selected backend".into(),
                    });
                }
            };
            let staging_bytes = staged.staged_bytes();
            if let Some(reason) = scope.context().stop_reason() {
                return Err(MimeError::OperationStopped(Box::new(StopReport {
                    reason,
                    accepted_at: None,
                })));
            }
            let command = ManagedMimeCommand::new(
                self.command_runner.clone(),
                Self::command_for_path(&staged.path()?),
                services.root_group.clone(),
            );
            #[cfg(test)]
            let output = if let Some(executor) = &self.test_command_executor {
                executor.run_in(scope, command, staged).await?
            } else {
                let executor = if let Some(runner_invocations) = &self.test_runner_invocations {
                    ManagedCommandExecutor::with_runner_invocations(worker, Arc::clone(runner_invocations))
                } else {
                    ManagedCommandExecutor::new(worker)
                };
                executor.run_in(scope, command, staged).await?
            };
            #[cfg(not(test))]
            let output = ManagedCommandExecutor::new(worker)
                .run_in(scope, command, staged)
                .await?;
            if output.exit_code != Some(0) {
                return Err(MimeError::detector_backend(
                    Self::COMMAND,
                    format!("file exited with code {:?}", output.exit_code),
                ));
            }
            let value = require_complete_stdout(&output)
                .map_err(|reason| MimeError::detector_backend(Self::COMMAND, reason))?
                .trim();
            let candidates = if value.is_empty() {
                Vec::new()
            } else if is_mime_name(value) {
                vec![MimeCandidate {
                    mime_type: value.to_owned(),
                    rank: 0,
                }]
            } else {
                return Err(MimeError::detector_backend(
                    Self::COMMAND,
                    "file returned an invalid MIME type",
                ));
            };
            Ok(MimeCandidates {
                candidates,
                inspected_extent: InspectedExtent::Complete,
                retained_prefix: Vec::new(),
                staging_bytes,
            })
        })
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
    ///
    /// # Returns
    /// A file command detector configured with the embedded repository and
    /// default command runner.
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> FileBasedMimeDetector for FileCommandMimeDetector<'a> {
    /// Gets the shared detector core.
    ///
    /// # Returns
    /// Shared detector core used for configuration and candidate processing.
    fn core(&self) -> &MimeDetectorCore {
        &self.core
    }

    /// Gets the maximum content prefix length used when staging a reader.
    ///
    /// The prefix budget comes from [`MimeConfig::max_buffer_size`] and is
    /// independent of the magic offsets in the filename repository.
    ///
    /// # Returns
    /// Maximum byte prefix used for content inspection.
    fn max_test_bytes(&self) -> usize {
        self.core.max_buffer_size()
    }

    /// Guesses MIME type names from filename rules.
    ///
    /// # Parameters
    /// - `filename`: File path or basename to match.
    ///
    /// # Returns
    /// Matching MIME type names in repository order.
    fn guess_from_filename(&self, filename: &str) -> Vec<String> {
        FileCommandMimeDetector::guess_from_filename(self, filename)
    }

    /// Guesses filename candidates from an already extracted native basename.
    ///
    /// # Parameters
    /// - `basename`: Native basename whose filename rules are matched.
    ///
    /// # Returns
    /// Candidate MIME type names in repository order.
    fn guess_from_filename_basename(&self, basename: &str) -> Vec<String> {
        FileCommandMimeDetector::guess_from_filename_basename(self, basename)
    }

    /// Guesses MIME type names from a local file using the file command.
    ///
    /// # Parameters
    /// - `file`: Local file path passed to the `file` command.
    ///
    /// # Returns
    /// MIME type names reported by the command.
    ///
    /// # Errors
    /// Returns an error when the command cannot inspect the file or its output
    /// cannot be parsed as MIME type names.
    fn guess_from_local_file(&self, file: &Path) -> MimeResult<Vec<String>> {
        self.guess_from_file_command(file)
    }
}

#[cfg(test)]
mod file_operation_tests {
    use std::ffi::OsString;
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;
    use std::task::Context;
    use std::task::Poll;
    use std::task::Wake;
    use std::task::Waker;

    use qubit_fs::AsyncFileSystem;
    use qubit_fs::FsResult;
    use qubit_fs::Path as FsPath;
    use qubit_fs::metadata::FileSystemCapabilities;
    use qubit_fs::metadata::FileSystemCapability;
    use qubit_fs::metadata::FileSystemId;
    use qubit_fs::metadata::FileSystemInfo;
    use qubit_fs::metadata::FileSystemLimits;
    use qubit_fs::metadata::OpenedFileInfo;
    use qubit_fs::metadata::SymlinkPolicy;
    use qubit_fs::path::PathConstraints;
    use qubit_fs::path::PathSemantics;
    use qubit_fs::spi::AsyncFileSystemSpi;
    use qubit_fs::spi::OpenReaderRequest;
    use qubit_fs::spi::OpenedAsyncReader;
    use qubit_fs::spi::ProviderOperation;
    use qubit_fs::spi::ProviderOperations;
    use qubit_fs::spi::ProviderProperties;
    use qubit_fs::spi::SpiFuture;
    use qubit_fs::spi::StatRequest;
    use qubit_fs::spi::StatResponse;
    use qubit_io::AsyncInput;
    use qubit_operation::AsyncOperationHandle;
    use qubit_operation::GroupOptions;
    use qubit_operation::OperationContext;
    use qubit_operation::OperationGroup;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOutcome;
    use qubit_operation::OperationScope;
    use qubit_operation::ResourceRef;
    use qubit_operation::SettlementStatus;
    use qubit_operation::StopReason;
    use qubit_operation::StopReport;

    use super::AsyncMimeCommandExecutor;
    use super::AsyncMimeDetectorBackend;
    use super::FileCommandMimeDetector;
    use super::ManagedMimeCommand;
    use super::MimeCandidates;
    use super::MimeError;
    use super::MimeExecutionServices;
    use super::MimeFuture;
    use super::MimeResult;
    use crate::BorrowedMimeInput;
    use crate::InputExtent;
    use crate::MimeExecutionPort;
    use crate::MimeOperationOptions;
    use crate::command_execution::MimeCommandOutput;
    use crate::command_execution::MimeExecutionWorker;
    use crate::managed_temp_staging::ManagedStagingLimits;
    use crate::managed_temp_staging::StagedMimeInput;
    use crate::managed_temp_staging::stage_borrowed_bytes_in;

    struct ThreadWake(std::thread::Thread);
    impl Wake for ThreadWake {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
        fn wake_by_ref(self: &Arc<Self>) {
            self.0.unpark();
        }
    }

    fn block_on<F: Future>(future: F) -> F::Output {
        let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
        let mut context = Context::from_waker(&waker);
        let mut future = std::pin::pin!(future);
        loop {
            match future.as_mut().poll(&mut context) {
                Poll::Ready(value) => return value,
                Poll::Pending => std::thread::park(),
            }
        }
    }

    struct ImmediatePort;
    impl std::fmt::Debug for ImmediatePort {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("ImmediatePort")
        }
    }

    struct ProviderFs {
        properties: ProviderProperties,
        bytes: Vec<u8>,
        reads: Arc<AtomicUsize>,
        stop_context: Option<Arc<std::sync::Mutex<Option<OperationContext>>>>,
        first_read_hook: Option<Arc<dyn Fn() + Send + Sync>>,
    }

    type ProviderStopContext = Arc<std::sync::Mutex<Option<OperationContext>>>;
    type ProviderBuild = (Arc<AsyncFileSystem>, Arc<AtomicUsize>, Option<ProviderStopContext>);
    type RecordedCommand = (std::path::PathBuf, Vec<OsString>, String);

    impl ProviderFs {
        fn filesystem(bytes: &[u8]) -> (Arc<AsyncFileSystem>, Arc<AtomicUsize>) {
            let (filesystem, reads, _) = Self::with_optional_stop(bytes, None, None);
            (filesystem, reads)
        }

        fn stopping(bytes: &[u8]) -> (Arc<AsyncFileSystem>, Arc<AtomicUsize>, ProviderStopContext) {
            let stop_context = Arc::new(std::sync::Mutex::new(None));
            let (filesystem, reads, stop_context) = Self::with_optional_stop(bytes, Some(stop_context), None);
            (filesystem, reads, stop_context.unwrap())
        }

        fn deadline_after_first_read(bytes: &[u8], hook: Arc<dyn Fn() + Send + Sync>) -> (Arc<AsyncFileSystem>, Arc<AtomicUsize>) {
            let (filesystem, reads, _) = Self::with_optional_stop(bytes, None, Some(hook));
            (filesystem, reads)
        }

        fn with_optional_stop(bytes: &[u8], stop_context: Option<ProviderStopContext>, first_read_hook: Option<Arc<dyn Fn() + Send + Sync>>) -> ProviderBuild {
            let properties = ProviderProperties::new(
                FileSystemInfo::new(
                    FileSystemId::new("mime-file-operation-test").unwrap(),
                    "mime-file-operation-test",
                    PathSemantics::Hierarchical,
                ),
                ProviderOperations::new().with(ProviderOperation::OpenReader),
                FileSystemCapabilities::new().with_guaranteed(FileSystemCapability::Read),
                FileSystemLimits::unknown(),
                PathConstraints::absolute(),
                SymlinkPolicy::Reject,
            )
            .unwrap();
            let reads = Arc::new(AtomicUsize::new(0));
            let filesystem = AsyncFileSystem::from_spi(Self {
                properties,
                bytes: bytes.to_vec(),
                reads: Arc::clone(&reads),
                stop_context: stop_context.clone(),
                first_read_hook,
            })
            .unwrap();
            (Arc::new(filesystem), reads, stop_context)
        }
    }

    impl AsyncFileSystemSpi for ProviderFs {
        fn properties(&self) -> ProviderProperties {
            self.properties.clone()
        }

        fn stat<'a>(&'a self, _request: StatRequest<'a>) -> SpiFuture<'a, FsResult<StatResponse>> {
            Box::pin(async { panic!("file staging must not stat provider input") })
        }

        fn open_reader<'a>(&'a self, request: OpenReaderRequest<'a>) -> SpiFuture<'a, FsResult<OpenedAsyncReader>> {
            let id = FileSystemId::new("mime-file-operation-test").unwrap();
            let info = OpenedFileInfo::new(id, request.path().clone());
            let bytes = self.bytes.clone();
            let reads = Arc::clone(&self.reads);
            let stop_context = self.stop_context.as_ref().map(Arc::clone);
            let first_read_hook = self.first_read_hook.as_ref().map(Arc::clone);
            Box::pin(async move {
                Ok(OpenedAsyncReader::new(
                    info,
                    Box::new(ProviderInput {
                        bytes,
                        offset: 0,
                        reads,
                        stop_context,
                        first_read_hook,
                    }),
                ))
            })
        }
    }

    struct ProviderInput {
        bytes: Vec<u8>,
        offset: usize,
        reads: Arc<AtomicUsize>,
        stop_context: Option<Arc<std::sync::Mutex<Option<OperationContext>>>>,
        first_read_hook: Option<Arc<dyn Fn() + Send + Sync>>,
    }

    impl AsyncInput for ProviderInput {
        type Item = u8;
        unsafe fn poll_read_unchecked(
            mut self: Pin<&mut Self>,
            _context: &mut Context<'_>,
            output: &mut [u8],
            index: usize,
            count: usize,
        ) -> Poll<std::io::Result<usize>> {
            let first_read = self.reads.fetch_add(1, Ordering::SeqCst) == 0;
            let count = count.min(self.bytes.len().saturating_sub(self.offset));
            output[index..index + count].copy_from_slice(&self.bytes[self.offset..self.offset + count]);
            self.offset += count;
            if let Some(context) = self.stop_context.as_ref().and_then(|slot| slot.lock().unwrap().take()) {
                let _ = context.request_stop(StopReason::UserCancelled);
            }
            if first_read && let Some(hook) = self.first_read_hook.take() {
                hook();
            }
            Poll::Ready(Ok(count))
        }
    }
    impl MimeExecutionPort for ImmediatePort {
        fn prepare(&self) -> Result<(), MimeError> {
            Ok(())
        }
        fn try_dispatch(
            &self,
            job: Box<dyn FnOnce() + Send + 'static>,
        ) -> Result<(), Box<dyn FnOnce() + Send + 'static>> {
            job();
            Ok(())
        }
        fn shutdown(&self) -> Result<(), MimeError> {
            Ok(())
        }
    }

    struct RecordingExecutor {
        output: std::sync::Mutex<Option<MimeResult<MimeCommandOutput>>>,
        calls: AtomicUsize,
        observation: Arc<std::sync::Mutex<Option<RecordedCommand>>>,
        helper_ref: Option<Arc<std::sync::Mutex<Option<ResourceRef<StagedMimeInput>>>>>,
    }
    impl std::fmt::Debug for RecordingExecutor {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("RecordingExecutor")
        }
    }
    impl AsyncMimeCommandExecutor for RecordingExecutor {
        fn run_in<'a>(
            &'a self,
            _scope: &'a OperationScope,
            request: ManagedMimeCommand,
            input: ResourceRef<StagedMimeInput>,
        ) -> MimeFuture<'a, MimeCommandOutput> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let path = input.path().expect("staged path should be available");
            let bytes = std::fs::read(&path).expect("staged bytes should be readable");
            *self.observation.lock().unwrap() = Some((
                path,
                request.command.arguments().map(OsString::from).collect(),
                String::from_utf8(bytes).unwrap(),
            ));
            if let Some(helper_ref) = &self.helper_ref {
                *helper_ref.lock().unwrap() = Some(input.clone());
            }
            let output = self.output.lock().unwrap().take().expect("one output per call");
            Box::pin(async move { output })
        }
    }

    struct Harness {
        detector: FileCommandMimeDetector<'static>,
        services: MimeExecutionServices,
        group: OperationGroup,
    }

    fn harness(executor: Arc<RecordingExecutor>) -> Harness {
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 4 }).unwrap();
        let services =
            MimeExecutionServices::new(manager.clone(), group.clone(), Arc::new(ImmediatePort), 1024).unwrap();
        services.prepare().unwrap();
        Harness {
            detector: FileCommandMimeDetector::new()
                .with_execution_services(services.clone())
                .with_test_command_executor(executor),
            services,
            group,
        }
    }

    fn executor(
        stdout: &[u8],
        truncated: bool,
        helper_ref: Option<Arc<std::sync::Mutex<Option<ResourceRef<StagedMimeInput>>>>>,
    ) -> Arc<RecordingExecutor> {
        Arc::new(RecordingExecutor {
            output: std::sync::Mutex::new(Some(Ok(MimeCommandOutput {
                exit_code: Some(0),
                stdout: stdout.to_vec(),
                stdout_truncated: truncated,
                stdout_complete: !truncated,
            }))),
            calls: AtomicUsize::new(0),
            observation: Arc::new(std::sync::Mutex::new(None)),
            helper_ref,
        })
    }

    fn submit(
        harness: &Harness,
        bytes: Arc<[u8]>,
        options: MimeOperationOptions,
        cancelled: bool,
    ) -> (
        AsyncOperationHandle<MimeCandidates, MimeError>,
        OperationOutcome<MimeCandidates, MimeError>,
    ) {
        let detector = harness.detector.clone();
        let services = harness.services.clone();
        let tree = services.new_read_tree(0);
        let mut handle = harness
            .group
            .as_async()
            .try_submit(Default::default(), move |scope| async move {
                services.bind_read_tree(scope.id(), &tree);
                if cancelled {
                    let _ = scope.request_stop(StopReason::UserCancelled);
                }
                detector
                    .guess_content_in(
                        &scope,
                        BorrowedMimeInput::Bytes {
                            content: &bytes,
                            extent: InputExtent::Complete,
                            filename: Some("--private-name.bin"),
                        },
                        &options,
                    )
                    .await
            })
            .unwrap();
        let outcome = block_on(handle.wait_outcome()).unwrap();
        (handle, outcome)
    }

    fn submit_provider(
        harness: &Harness,
        filesystem: Arc<AsyncFileSystem>,
        options: MimeOperationOptions,
        stop_context: Option<Arc<std::sync::Mutex<Option<OperationContext>>>>,
        operation_options: qubit_operation::OperationOptions,
    ) -> (
        AsyncOperationHandle<MimeCandidates, MimeError>,
        OperationOutcome<MimeCandidates, MimeError>,
        Arc<crate::detector::ReadTreeContext>,
    ) {
        let detector = harness.detector.clone();
        let services = harness.services.clone();
        let tree = services.new_read_tree(1024);
        let tree_result = Arc::clone(&tree);
        let mut handle = harness
            .group
            .as_async()
            .try_submit(operation_options, move |scope| async move {
                services.bind_read_tree(scope.id(), &tree);
                if let Some(stop_context) = stop_context {
                    *stop_context.lock().unwrap() = Some(scope.context().clone());
                }
                let path = FsPath::parse("/input").unwrap();
                detector
                    .guess_content_in(
                        &scope,
                        BorrowedMimeInput::Provider {
                            file_system: &filesystem,
                            path: &path,
                        },
                        &options,
                    )
                    .await
            })
            .unwrap();
        let outcome = block_on(handle.wait_outcome()).unwrap();
        (handle, outcome, tree_result)
    }

    fn cleanup(harness: &Harness) {
        let shutdown = harness.services.shutdown();
        let _ = block_on(shutdown.wait_services_settled()).unwrap();
        let _ = shutdown.stop_services().unwrap();
    }

    #[test]
    fn test_file_operation_staging_stopped_before_create_dispatches_no_work() {
        #[derive(Debug)]
        struct CountingPort(Arc<AtomicUsize>);
        impl MimeExecutionPort for CountingPort {
            fn prepare(&self) -> Result<(), MimeError> {
                Ok(())
            }
            fn try_dispatch(
                &self,
                job: Box<dyn FnOnce() + Send + 'static>,
            ) -> Result<(), Box<dyn FnOnce() + Send + 'static>> {
                self.0.fetch_add(1, Ordering::SeqCst);
                job();
                Ok(())
            }
            fn shutdown(&self) -> Result<(), MimeError> {
                Ok(())
            }
        }

        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 2 }).unwrap();
        let dispatches = Arc::new(AtomicUsize::new(0));
        let port = Arc::new(CountingPort(Arc::clone(&dispatches)));
        let services = MimeExecutionServices::new(manager, group.clone(), port.clone(), 16).unwrap();
        services.prepare().unwrap();
        let worker = MimeExecutionWorker::new(port);
        let budget = services.staging_budget();
        let limits = ManagedStagingLimits {
            max_bytes: 16,
            budget: budget.clone(),
            worker,
        };
        let mut handle = group
            .as_async()
            .try_submit(Default::default(), move |scope| async move {
                let _ = scope.request_stop(StopReason::UserCancelled);
                stage_borrowed_bytes_in(&scope, b"input", InputExtent::Complete, limits)
                    .await
                    .map(|_| ())
            })
            .unwrap();
        assert!(matches!(
            block_on(handle.wait_outcome()).unwrap(),
            OperationOutcome::Stopped(_)
        ));
        assert_eq!(dispatches.load(Ordering::SeqCst), 0);
        assert_eq!(budget.used_bytes(), 0);
        assert!(block_on(handle.wait_settlement()).all_released());
        let shutdown = services.shutdown();
        let _ = block_on(shutdown.wait_services_settled()).unwrap();
        let _ = shutdown.stop_services().unwrap();
    }

    #[test]
    fn test_file_operation_staging_deadline_before_create_dispatches_no_work() {
        struct FakeClock {
            domain: qubit_operation::ClockDomainId,
            ticks: Arc<std::sync::atomic::AtomicU64>,
        }
        impl qubit_operation::ClockSource for FakeClock {
            fn now(&self) -> qubit_operation::MonotonicTime {
                qubit_operation::MonotonicTime::new(self.domain, self.ticks.load(Ordering::Acquire))
            }
            fn register_deadline(
                &self,
                _at: qubit_operation::MonotonicTime,
                _wake: Waker,
            ) -> Box<dyn qubit_operation::TimerRegistration> {
                Box::new(TestTimerRegistration)
            }
        }
        struct TestTimerRegistration;
        impl qubit_operation::TimerRegistration for TestTimerRegistration {
            fn cancel(self: Box<Self>) {}
        }

        #[derive(Debug)]
        struct CountingPort(Arc<AtomicUsize>);
        impl MimeExecutionPort for CountingPort {
            fn prepare(&self) -> Result<(), MimeError> { Ok(()) }
            fn try_dispatch(&self, job: Box<dyn FnOnce() + Send + 'static>) -> Result<(), Box<dyn FnOnce() + Send + 'static>> {
                self.0.fetch_add(1, Ordering::SeqCst);
                job();
                Ok(())
            }
            fn shutdown(&self) -> Result<(), MimeError> { Ok(()) }
        }

        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 2 }).unwrap();
        let dispatches = Arc::new(AtomicUsize::new(0));
        let port = Arc::new(CountingPort(Arc::clone(&dispatches)));
        let services = MimeExecutionServices::new(manager, group.clone(), port.clone(), 16).unwrap();
        services.prepare().unwrap();
        let limits = ManagedStagingLimits {
            max_bytes: 16,
            budget: services.staging_budget(),
            worker: MimeExecutionWorker::new(port),
        };
        let domain = qubit_operation::ClockDomainId::new_unique();
        let ticks = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let clock = Arc::new(FakeClock { domain, ticks: Arc::clone(&ticks) });
        let deadline_ticks = Arc::clone(&ticks);
        let mut handle = group.as_async().try_submit(
            qubit_operation::OperationOptions {
                context: qubit_operation::ContextOptions {
                    clock: Some(clock),
                    deadline: Some(qubit_operation::Deadline::at(qubit_operation::MonotonicTime::new(domain, 5))),
                    ..Default::default()
                },
                ..Default::default()
            },
            move |scope| async move {
                deadline_ticks.store(5, Ordering::Release);
                stage_borrowed_bytes_in(&scope, b"input", InputExtent::Complete, limits).await.map(|_| ())
            },
        ).unwrap();
        let outcome = block_on(handle.wait_outcome()).unwrap();
        assert!(matches!(outcome, OperationOutcome::Failed(MimeError::OperationStopped(ref report)) if report.reason == StopReason::Timeout), "expired staging outcome: {outcome:?}");
        assert_eq!(0, dispatches.load(Ordering::SeqCst), "expired staging must not dispatch create or write work");
        assert_eq!(0, services.staging_budget().used_bytes());
        assert!(block_on(handle.wait_settlement()).all_released());
        let shutdown = services.shutdown();
        let _ = block_on(shutdown.wait_services_settled()).unwrap();
        let _ = shutdown.stop_services().unwrap();
    }

    #[test]
    fn test_file_operation_staging_deadline_after_first_provider_read_blocks_next_read() {
        struct ReadDeadlineClock {
            domain: qubit_operation::ClockDomainId,
            ticks: Arc<std::sync::atomic::AtomicU64>,
        }
        impl qubit_operation::ClockSource for ReadDeadlineClock {
            fn now(&self) -> qubit_operation::MonotonicTime {
                qubit_operation::MonotonicTime::new(self.domain, self.ticks.load(Ordering::Acquire))
            }
            fn register_deadline(&self, _at: qubit_operation::MonotonicTime, _wake: Waker) -> Box<dyn qubit_operation::TimerRegistration> {
                Box::new(ReadDeadlineRegistration)
            }
        }
        struct ReadDeadlineRegistration;
        impl qubit_operation::TimerRegistration for ReadDeadlineRegistration {
            fn cancel(self: Box<Self>) {}
        }

        let fake = executor(b"application/pdf", false, None);
        let harness = harness(Arc::clone(&fake));
        let domain = qubit_operation::ClockDomainId::new_unique();
        let ticks = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let tick_after_read = Arc::clone(&ticks);
        let hook: Arc<dyn Fn() + Send + Sync> = Arc::new(move || tick_after_read.store(5, Ordering::Release));
        let (filesystem, reads) = ProviderFs::deadline_after_first_read(&[b'x'; 32], hook);
        let operation_options = qubit_operation::OperationOptions {
            context: qubit_operation::ContextOptions {
                clock: Some(Arc::new(ReadDeadlineClock { domain, ticks })),
                deadline: Some(qubit_operation::Deadline::at(qubit_operation::MonotonicTime::new(domain, 5))),
                ..Default::default()
            },
            ..Default::default()
        };
        let options = MimeOperationOptions { max_staging_bytes: Some(8), ..Default::default() };
        let (handle, outcome, read_tree) = submit_provider(&harness, filesystem, options, None, operation_options);
        assert!(matches!(outcome, OperationOutcome::Failed(MimeError::OperationStopped(ref report)) if report.reason == StopReason::Timeout));
        assert_eq!(1, reads.load(Ordering::SeqCst), "deadline after the successful first read blocks the next Provider read");
        assert!(read_tree.consumed() > 0, "successful first read remains charged");
        assert_eq!(0, fake.calls.load(Ordering::SeqCst), "command execution must not start after the staging deadline");
        assert_eq!(0, harness.services.staging_budget().used_bytes());
        assert!(block_on(handle.wait_settlement()).all_released());
        cleanup(&harness);
    }

    #[test]
    fn test_file_operation_cancel_before_launch() {
        let fake = executor(b"application/pdf", false, None);
        let harness = harness(Arc::clone(&fake));
        let (handle, outcome) = submit(
            &harness,
            Arc::from(b"complete input".as_slice()),
            MimeOperationOptions::default(),
            true,
        );
        assert!(matches!(outcome, OperationOutcome::Stopped(_)));
        assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
        block_on(handle.wait_settlement());
        cleanup(&harness);
    }

    #[test]
    fn test_file_operation_staged_input_survives_helpers() {
        let held = Arc::new(std::sync::Mutex::new(None));
        let fake = executor(b"application/pdf\n", false, Some(Arc::clone(&held)));
        let harness_empty = harness(Arc::clone(&fake));
        let (handle, outcome) = submit(
            &harness_empty,
            Arc::from(b"full object".as_slice()),
            MimeOperationOptions::default(),
            false,
        );
        assert!(matches!(outcome, OperationOutcome::Succeeded(_)));
        if let OperationOutcome::Succeeded(result) = &outcome {
            assert_eq!(result.candidates[0].mime_type, "application/pdf");
        }
        let resource = held.lock().unwrap().take().unwrap();
        let path = resource.path().unwrap();
        assert!(path.exists());
        assert_eq!(std::fs::read(&path).unwrap(), b"full object");
        drop(resource);
        assert!(block_on(handle.wait_settlement()).all_released());
        assert!(!path.exists());
        cleanup(&harness_empty);
    }

    #[test]
    fn test_file_operation_structured_argv_and_private_temp_path() {
        let fake = executor(b"application/pdf\n", false, None);
        let observation = Arc::clone(&fake.observation);
        let harness = harness(fake.clone());
        let (handle, outcome) = submit(
            &harness,
            Arc::from(b"complete".as_slice()),
            MimeOperationOptions::default(),
            false,
        );
        assert!(matches!(outcome, OperationOutcome::Succeeded(_)));
        let (path, args, _) = observation.lock().unwrap().take().unwrap();
        assert_eq!(args[0], "--mime-type");
        assert_eq!(args[1], "--brief");
        assert_eq!(args[2], "--");
        assert_eq!(args[3], path.as_os_str());
        assert!(path.is_absolute());
        let command = FileCommandMimeDetector::command_for_path(&path);
        assert!(!format!("{command:?}").contains(path.to_string_lossy().as_ref()));
        block_on(handle.wait_settlement());
        assert!(!path.exists());
        cleanup(&harness);
    }

    #[test]
    fn test_file_operation_output_policy_checks_truncation_and_empty_result() {
        let fake = executor(b"application/pdf", true, None);
        let harness_truncated = harness(Arc::clone(&fake));
        let (handle, outcome) = submit(
            &harness_truncated,
            Arc::from(b"complete".as_slice()),
            MimeOperationOptions::default(),
            false,
        );
        assert!(matches!(
            outcome,
            OperationOutcome::Failed(MimeError::DetectorBackend { .. })
        ));
        block_on(handle.wait_settlement());
        cleanup(&harness_truncated);

        let fake = executor(b"  \n", false, None);
        let harness_empty = harness(Arc::clone(&fake));
        let (handle, outcome) = submit(
            &harness_empty,
            Arc::from(b"complete".as_slice()),
            MimeOperationOptions::default(),
            false,
        );
        assert!(matches!(outcome, OperationOutcome::Succeeded(ref result) if result.candidates.is_empty()));
        block_on(handle.wait_settlement());
        cleanup(&harness_empty);
    }

    #[test]
    fn test_file_operation_budget_rejects_before_launch_and_zero_options() {
        let fake = executor(b"application/pdf", false, None);
        let harness = harness(Arc::clone(&fake));
        let options = MimeOperationOptions {
            max_staging_bytes: Some(4),
            ..Default::default()
        };
        let (handle, outcome) = submit(&harness, Arc::from(b"five!".as_slice()), options, false);
        assert!(matches!(
            outcome,
            OperationOutcome::Failed(MimeError::BudgetExceeded { kind: "staging", .. })
        ));
        assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
        block_on(handle.wait_settlement());

        let options = MimeOperationOptions {
            max_staging_bytes: Some(0),
            ..Default::default()
        };
        let (handle, outcome) = submit(&harness, Arc::from(b"data".as_slice()), options, false);
        assert!(matches!(
            outcome,
            OperationOutcome::Failed(MimeError::InvalidOperationBudget)
        ));
        assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
        block_on(handle.wait_settlement());
        cleanup(&harness);
    }

    #[test]
    fn test_file_operation_provider_sentinel_rejects_l_plus_one_before_launch() {
        let fake = executor(b"application/pdf", false, None);
        let harness = harness(Arc::clone(&fake));
        let (filesystem, reads) = ProviderFs::filesystem(b"12345");
        let options = MimeOperationOptions {
            max_staging_bytes: Some(4),
            ..Default::default()
        };
        let (handle, outcome, _read_tree) = submit_provider(&harness, filesystem, options, None, Default::default());
        assert!(matches!(
            outcome,
            OperationOutcome::Failed(MimeError::BudgetExceeded {
                kind: "staging",
                requested: 5,
                limit: 4,
            })
        ));
        assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
        assert_eq!(reads.load(Ordering::SeqCst), 2, "data and L+1 sentinel are read");
        block_on(handle.wait_settlement());
        cleanup(&harness);
    }

    #[test]
    fn test_file_operation_provider_stop_after_read_commits_bytes_and_prevents_next_read() {
        let fake = executor(b"application/pdf", false, None);
        let harness = harness(Arc::clone(&fake));
        let (filesystem, reads, stop_context) = ProviderFs::stopping(&[b'x'; 32]);
        let options = MimeOperationOptions {
            max_staging_bytes: Some(2),
            ..Default::default()
        };
        let (handle, outcome, read_tree) = submit_provider(&harness, filesystem, options, Some(stop_context), Default::default());
        assert!(matches!(outcome, OperationOutcome::Stopped(_)));
        assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
        assert_eq!(reads.load(Ordering::SeqCst), 1, "no read begins after stop");
        assert_eq!(read_tree.remaining(), 1022, "the returned bytes remain charged");
        assert!(block_on(handle.wait_settlement()).all_released());
        assert_eq!(harness.services.staging_budget().used_bytes(), 0);
        cleanup(&harness);
    }

    #[test]
    fn test_file_operation_executor_error_keeps_typed_timeout_error() {
        let held = Arc::new(std::sync::Mutex::new(None));
        let fake = Arc::new(RecordingExecutor {
            output: std::sync::Mutex::new(Some(Err(MimeError::OperationStopped(Box::new(StopReport {
                reason: StopReason::Timeout,
                accepted_at: None,
            }))))),
            calls: AtomicUsize::new(0),
            observation: Arc::new(std::sync::Mutex::new(None)),
            helper_ref: Some(Arc::clone(&held)),
        });
        let harness = harness(Arc::clone(&fake));
        let (handle, outcome) = submit(
            &harness,
            Arc::from(b"complete".as_slice()),
            MimeOperationOptions::default(),
            false,
        );
        assert!(matches!(
            outcome,
            OperationOutcome::Failed(MimeError::OperationStopped(report))
                if report.reason == StopReason::Timeout
        ));
        assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
        let resource = held.lock().unwrap().take().unwrap();
        let path = resource.path().unwrap();
        assert!(path.exists(), "helper reference keeps the staged file alive");
        let sandbox = path.parent().unwrap().to_path_buf();
        let blocker = sandbox.join("command-cleanup-blocker");
        std::fs::write(&blocker, b"force retryable cleanup failure").unwrap();
        drop(resource);
        let settlement = block_on(handle.wait_settlement());
        assert_eq!(
            settlement.status,
            SettlementStatus::RecoveryRequired,
            "cleanup failure is reported separately from the primary timeout"
        );
        assert!(!path.exists());
        std::fs::remove_file(blocker).unwrap();
        let retry = settlement.recoveries[0].try_retry().unwrap();
        assert!(block_on(retry.wait_async()).failures.is_empty());
        assert!(!sandbox.exists());
        cleanup(&harness);
    }

    #[test]
    fn test_file_operation_runs_managed_command_through_fixed_worker() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 1 }).unwrap();
        let port = Arc::new(crate::FixedMimeExecutionPort::new(1, 4));
        let services = MimeExecutionServices::new(manager.clone(), group.clone(), port, 1024).unwrap();
        services.prepare().unwrap();
        let detector = FileCommandMimeDetector::new()
            .with_command_runner(qubit_command::CommandRunner::new(std::time::Duration::from_secs(5)))
            .with_execution_services(services.clone());
        assert!(services.is_prepared());
        assert!(detector.execution_services.is_some());
        let content: Arc<[u8]> = Arc::from(&b"hello managed worker\n"[..]);
        let content_len = content.len() as u64;
        let mut operation = group
            .as_async()
            .try_submit(Default::default(), move |scope| async move {
                detector
                    .guess_content_in(
                        &scope,
                        BorrowedMimeInput::Bytes {
                            content: &content,
                            extent: InputExtent::Complete,
                            filename: None,
                        },
                        &MimeOperationOptions::default(),
                    )
                    .await
            })
            .unwrap();
        let outcome = block_on(operation.wait_outcome()).unwrap();
        assert!(
            matches!(&outcome, OperationOutcome::Succeeded(result)
            if result.candidates.iter().any(|candidate| candidate.mime_type == "text/plain")
                && result.staging_bytes == content_len),
            "managed file command failed or returned an unexpected result: {outcome:?}"
        );
        let settlement = block_on(operation.wait_settlement());
        assert_eq!(settlement.status, SettlementStatus::Released);
        let shutdown = services.shutdown();
        let _ = block_on(shutdown.wait_services_settled()).unwrap();
        let _ = shutdown.stop_services().unwrap();
        let _report = block_on(shutdown.wait_group_shutdown()).unwrap();
    }

    #[test]
    fn test_file_command_child_is_admitted_after_parent_group_drain() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 1 }).unwrap();
        let port = Arc::new(crate::FixedMimeExecutionPort::new(1, 2));
        let services = MimeExecutionServices::new(manager.clone(), group.clone(), port, 1024).unwrap();
        services.prepare().unwrap();
        let detector = FileCommandMimeDetector::new()
            .with_command_runner(qubit_command::CommandRunner::new(std::time::Duration::from_secs(5)))
            .with_execution_services(services.clone());
        let content: Arc<[u8]> = Arc::from(&b"hello managed drain\n"[..]);
        let content_len = content.len() as u64;
        let drain_group = group.clone();
        let mut operation = group
            .as_async()
            .try_submit(Default::default(), move |scope| async move {
                drain_group.close_admission();
                detector
                    .guess_content_in(
                        &scope,
                        BorrowedMimeInput::Bytes {
                            content: &content,
                            extent: InputExtent::Complete,
                            filename: None,
                        },
                        &MimeOperationOptions::default(),
                    )
                    .await
            })
            .unwrap();
        let outcome = block_on(operation.wait_outcome()).unwrap();
        assert!(
            matches!(&outcome, OperationOutcome::Succeeded(result)
            if result.candidates.iter().any(|candidate| candidate.mime_type == "text/plain")
                && result.staging_bytes == content_len),
            "a child with an inherited group permit should survive Drain: {outcome:?}"
        );
        assert_eq!(SettlementStatus::Released, block_on(operation.wait_settlement()).status);
        let shutdown = services.shutdown();
        let _ = block_on(shutdown.wait_services_settled()).unwrap();
        let _ = shutdown.stop_services().unwrap();
        let _report = block_on(shutdown.wait_group_shutdown()).unwrap();
    }

    #[test]
    fn test_full_command_queue_rejects_staging_before_command_dispatch() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 1 }).unwrap();
        let port = Arc::new(crate::FixedMimeExecutionPort::new(1, 1));
        let services = MimeExecutionServices::new(manager.clone(), group.clone(), port.clone(), 1024).unwrap();
        services.prepare().unwrap();
        let detector = FileCommandMimeDetector::new()
            .with_command_runner(qubit_command::CommandRunner::new(std::time::Duration::from_secs(5)))
            .with_execution_services(services.clone());
        assert!(services.is_prepared());
        assert!(detector.execution_services.is_some());

        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        assert!(
            port.try_dispatch(Box::new(move || {
                started_tx.send(()).unwrap();
                let _ = release_rx.recv();
            }))
            .is_ok()
        );
        started_rx.recv().unwrap();
        assert!(port.try_dispatch(Box::new(|| {})).is_ok());
        let full = port
            .try_dispatch_with_status(Box::new(|| {}))
            .expect_err("the second job must occupy the only queue slot");
        assert_eq!(crate::MimeExecutionDispatchFailureKind::QueueFull, full.kind());
        drop(full.into_job());

        let content: Arc<[u8]> = Arc::from(&b"queue pressure\n"[..]);
        let mut operation = group
            .as_async()
            .try_submit(Default::default(), move |scope| async move {
                detector
                    .guess_content_in(
                        &scope,
                        BorrowedMimeInput::Bytes {
                            content: &content,
                            extent: InputExtent::Complete,
                            filename: None,
                        },
                        &MimeOperationOptions::default(),
                    )
                    .await
            })
            .unwrap();
        let outcome = block_on(operation.wait_outcome()).unwrap();
        let queue_busy = matches!(outcome, OperationOutcome::Failed(crate::MimeError::CommandQueueBusy));
        release_tx.send(()).unwrap();
        let settlement = block_on(operation.wait_settlement());
        assert_eq!(SettlementStatus::Released, settlement.status);
        assert!(queue_busy, "unexpected managed command outcome: {outcome:?}");
        let shutdown = services.shutdown();
        let _ = block_on(shutdown.wait_services_settled()).unwrap();
        let _ = shutdown.stop_services().unwrap();
        let _report = block_on(shutdown.wait_group_shutdown()).unwrap();
    }

    #[test]
    fn test_command_dispatch_queue_full_after_staging_cleans_up_without_running_command() {
        #[derive(Debug, Default)]
        struct StageThenRejectPort {
            calls: AtomicUsize,
            events: std::sync::Mutex<Vec<&'static str>>,
        }
        impl MimeExecutionPort for StageThenRejectPort {
            fn prepare(&self) -> Result<(), MimeError> {
                Ok(())
            }
            fn shutdown(&self) -> Result<(), MimeError> {
                Ok(())
            }
            fn try_dispatch(
                &self,
                job: Box<dyn FnOnce() + Send + 'static>,
            ) -> Result<(), Box<dyn FnOnce() + Send + 'static>> {
                self.events.lock().unwrap().push("cleanup");
                job();
                Ok(())
            }
            fn try_dispatch_with_status(
                &self,
                job: Box<dyn FnOnce() + Send + 'static>,
            ) -> Result<(), crate::MimeExecutionDispatchFailure> {
                let call = self.calls.fetch_add(1, Ordering::SeqCst);
                if call < 2 {
                    self.events
                        .lock()
                        .unwrap()
                        .push(if call == 0 { "staging_create" } else { "staging_write" });
                    job();
                    Ok(())
                } else {
                    self.events.lock().unwrap().push("command_submit_rejected");
                    Err(crate::MimeExecutionDispatchFailure::new(
                        crate::MimeExecutionDispatchFailureKind::QueueFull,
                        job,
                    ))
                }
            }
        }

        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 2 }).unwrap();
        let port = Arc::new(StageThenRejectPort::default());
        let services = MimeExecutionServices::new(manager, group.clone(), port.clone(), 1024).unwrap();
        services.prepare().unwrap();
        let runner_invocations = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let detector = FileCommandMimeDetector::new()
            .with_execution_services(services.clone())
            .with_test_runner_invocations(Arc::clone(&runner_invocations));
        let input: Arc<[u8]> = Arc::from(b"staged file input".as_slice());
        let mut operation = group
            .as_async()
            .try_submit(Default::default(), move |scope| async move {
                detector
                    .guess_content_in(
                        &scope,
                        BorrowedMimeInput::Bytes {
                            content: &input,
                            extent: InputExtent::Complete,
                            filename: None,
                        },
                        &MimeOperationOptions::default(),
                    )
                    .await
            })
            .unwrap();
        let outcome = block_on(operation.wait_outcome()).unwrap();
        assert!(
            matches!(outcome, OperationOutcome::Failed(MimeError::CommandQueueBusy)),
            "{outcome:?}"
        );
        assert_eq!("staging_create", port.events.lock().unwrap()[0]);
        assert_eq!("staging_write", port.events.lock().unwrap()[1]);
        assert_eq!("command_submit_rejected", port.events.lock().unwrap()[2]);
        assert_eq!(
            0,
            runner_invocations.load(Ordering::SeqCst),
            "managed runner must not be invoked"
        );
        let settlement = block_on(operation.wait_settlement());
        assert!(
            settlement.all_released(),
            "staged file cleanup must settle: {settlement:?}"
        );
        assert_eq!(
            3,
            port.calls.load(Ordering::SeqCst),
            "staging and rejected command dispatches"
        );
        assert!(port.events.lock().unwrap()[3..].contains(&"cleanup"));
        assert_eq!(0, services.staging_budget().used_bytes());
        let shutdown = services.shutdown();
        let _ = block_on(shutdown.wait_services_settled()).unwrap();
        let _ = shutdown.stop_services().unwrap();
        let _ = block_on(shutdown.wait_group_shutdown()).unwrap();
    }
}
