// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! FFprobe-backed media stream classifier.

use std::path::Path;
use std::sync::Arc;

use qubit_command::Command;
use qubit_command::CommandRunner;

use crate::AsyncMediaStreamClassifierBackend;
use crate::BorrowedMimeInput;
use crate::FileBasedMediaStreamClassifier;
use crate::InputExtent;
use crate::MediaStreamType;
use crate::MimeConfig;
use crate::MimeError;
use crate::MimeFuture;
use crate::MimeOperationOptions;
use crate::MimeResult;
use crate::command_execution::AsyncMimeCommandExecutor;
use crate::command_execution::ManagedCommandExecutor;
use crate::command_execution::ManagedMimeCommand;
use crate::command_execution::MimeCommandExecutor;
use crate::command_execution::MimeExecutionWorker;
use crate::command_execution::SystemMimeCommandExecutor;
use crate::command_execution::require_complete_stdout;
use crate::detector::MimeExecutionServices;
use crate::detector::mime_input::OwnedMimeInput;
use crate::detector::stream_type_from_output;
use crate::managed_temp_staging::{ManagedStagingLimits, stage_owned_in};

/// Media stream classifier backed by the `ffprobe` command.
///
/// # Examples
///
/// ```
/// use qubit_mime::FfprobeCommandMediaStreamClassifier;
/// let classifier = FfprobeCommandMediaStreamClassifier::new();
/// assert!(classifier.max_staging_size() > 0);
/// ```
#[derive(Clone)]
pub struct FfprobeCommandMediaStreamClassifier {
    /// The working directory used to execute FFprobe.
    working_directory: Option<String>,
    /// The command runner used to execute FFprobe.
    command_runner: CommandRunner,
    /// Maximum bytes staged from reader/content input.
    max_staging_size: u64,
    /// Optional operation services for explicit asynchronous classification.
    execution_services: Option<Arc<MimeExecutionServices>>,
}

impl std::fmt::Debug for FfprobeCommandMediaStreamClassifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FfprobeCommandMediaStreamClassifier")
            .field("working_directory", &self.working_directory)
            .field("command_runner", &self.command_runner)
            .field("max_staging_size", &self.max_staging_size)
            .field("has_execution_services", &self.execution_services.is_some())
            .finish()
    }
}

impl FfprobeCommandMediaStreamClassifier {
    /// FFprobe executable name.
    pub const COMMAND: &'static str = "ffprobe";
    /// FFprobe stream name for video streams.
    pub const VIDEO_STREAM: &'static str = "video";
    /// FFprobe stream name for audio streams.
    pub const AUDIO_STREAM: &'static str = "audio";

    /// Creates a FFprobe-backed classifier.
    ///
    /// # Returns
    /// A classifier using the current process working directory.
    #[must_use]
    pub fn new() -> Self {
        Self::from_mime_config(MimeConfig::default())
    }

    /// Creates a FFprobe-backed classifier from MIME configuration.
    ///
    /// # Parameters
    /// - `config`: MIME configuration providing reader/content staging limits.
    ///
    /// # Returns
    /// A classifier using the current process working directory.
    #[must_use]
    pub fn from_mime_config(config: MimeConfig) -> Self {
        Self {
            working_directory: None,
            command_runner: Self::default_command_runner(&config),
            max_staging_size: config.media_stream_max_staging_size(),
            execution_services: None,
        }
    }

    /// Associates services required to run the asynchronous command path.
    pub(crate) fn with_execution_services(mut self, services: Arc<MimeExecutionServices>) -> Self {
        self.execution_services = Some(services);
        self
    }

    /// Gets the command runner used by this classifier.
    ///
    /// # Returns
    /// Runner used for `ffprobe` command executions.
    #[must_use = "the command runner must be inspected or configured"]
    #[inline]
    pub fn command_runner(&self) -> &CommandRunner {
        &self.command_runner
    }

    /// Gets the maximum bytes staged from reader/content input.
    ///
    /// # Returns
    /// Maximum accepted stream size before staging is rejected.
    #[must_use]
    #[inline]
    pub fn max_staging_size(&self) -> u64 {
        self.max_staging_size
    }

    /// Gets the configured working directory.
    ///
    /// # Returns
    /// Configured path when present, or `None` when FFprobe uses the process
    /// working directory.
    #[inline]
    pub fn working_directory(&self) -> Option<&str> {
        self.working_directory.as_deref()
    }

    /// Replaces the maximum bytes staged from reader/content input.
    ///
    /// # Parameters
    /// - `max_staging_size`: Maximum accepted stream size.
    #[inline]
    pub fn set_max_staging_size(&mut self, max_staging_size: u64) {
        self.max_staging_size = max_staging_size;
    }

    /// Replaces the command runner used by this classifier.
    ///
    /// # Parameters
    /// - `command_runner`: New runner configuration.
    #[inline]
    pub fn set_command_runner(&mut self, command_runner: CommandRunner) {
        self.command_runner = command_runner;
    }

    /// Sets the working directory used to execute FFprobe, or clears the
    /// override so the process working directory is used.
    ///
    /// # Parameters
    /// - `working_directory`: Path override, or `None` to clear the override.
    #[inline]
    pub fn set_working_directory(&mut self, working_directory: Option<String>) {
        self.working_directory = working_directory;
    }

    /// Replaces the maximum staging size and returns the updated classifier.
    ///
    /// # Parameters
    /// - `max_staging_size`: Maximum accepted stream size.
    ///
    /// # Returns
    /// The updated classifier.
    #[inline]
    #[must_use]
    pub fn with_max_staging_size(mut self, max_staging_size: u64) -> Self {
        self.max_staging_size = max_staging_size;
        self
    }

    /// Replaces the command runner and returns the updated classifier.
    ///
    /// # Parameters
    /// - `command_runner`: New runner configuration.
    ///
    /// # Returns
    /// The updated classifier.
    #[inline]
    #[must_use]
    pub fn with_command_runner(mut self, command_runner: CommandRunner) -> Self {
        self.command_runner = command_runner;
        self
    }

    /// Classifies FFprobe `codec_type` output.
    ///
    /// # Parameters
    /// - `output`: Lines printed by `ffprobe -show_entries stream=codec_type`.
    ///
    /// # Returns
    /// Media stream classification.
    #[must_use]
    #[inline]
    pub fn classify_stream_listing(output: &str) -> MediaStreamType {
        let has_video = output.lines().any(|line| line.trim() == Self::VIDEO_STREAM);
        let has_audio = output.lines().any(|line| line.trim() == Self::AUDIO_STREAM);
        match (has_video, has_audio) {
            (true, true) => MediaStreamType::VideoWithAudio,
            (true, false) => MediaStreamType::VideoOnly,
            (false, true) => MediaStreamType::AudioOnly,
            (false, false) => MediaStreamType::None,
        }
    }

    /// Checks whether the `ffprobe` command is available.
    ///
    /// Availability is checked by executing `ffprobe -version` with the default
    /// quiet command runner. The result only describes whether the command can
    /// be started successfully; a particular media file may still be unreadable
    /// or unsupported.
    ///
    /// # Returns
    /// `true` when `ffprobe -version` executes successfully.
    #[must_use]
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
    /// `true` when the `ffprobe` availability probe succeeds.
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
    pub(crate) fn is_available_with_executor(
        config: &MimeConfig,
        executor: &dyn MimeCommandExecutor,
    ) -> bool {
        executor
            .run(
                &Self::default_command_runner(config),
                Command::new(Self::COMMAND).arg("-version"),
            )
            .is_ok_and(|output| {
                output.exit_code == Some(0) && require_complete_stdout(&output).is_ok()
            })
    }

    /// Executes FFprobe for one local file.
    ///
    /// # Parameters
    /// - `path`: Local file path.
    ///
    /// # Returns
    /// Media stream classification. Non-zero FFprobe status is treated as
    /// [`MediaStreamType::None`] because stream refinement is best-effort.
    ///
    /// # Errors
    /// Returns [`MimeError::Command`](crate::MimeError::Command) when process
    /// execution itself fails.
    fn classify_with_ffprobe(&self, path: &Path) -> MimeResult<MediaStreamType> {
        self.classify_with_executor(path, &SystemMimeCommandExecutor)
    }

    /// Executes a local probe and interprets its actual exit status and stdout.
    ///
    /// # Parameters
    /// - `path`: Input path passed as a sensitive structured argument.
    /// - `executor`: Private execution boundary using this classifier's runner.
    ///
    /// # Returns
    /// Classification from complete UTF-8 stdout, or None for numeric nonzero
    /// exit.
    ///
    /// # Errors
    /// Propagates execution errors. Signal termination, truncated/incomplete
    /// stdout and invalid UTF-8 produce ClassifierBackend errors without
    /// captured data.
    pub(crate) fn classify_with_executor(
        &self,
        path: &Path,
        executor: &dyn MimeCommandExecutor,
    ) -> MimeResult<MediaStreamType> {
        let absolute_path = std::path::absolute(path)?;
        let mut command = Self::command_for_path(&absolute_path);
        if let Some(working_directory) = &self.working_directory {
            command = command.working_directory(working_directory);
        }
        let output = executor.run(&self.command_runner, command)?;
        match output.exit_code {
            Some(0) => {
                let stdout = require_complete_stdout(&output).map_err(|reason| {
                    MimeError::ClassifierBackend {
                        backend: Self::COMMAND.to_owned(),
                        reason: reason.to_owned(),
                    }
                })?;
                Ok(Self::classify_stream_listing(stdout))
            }
            Some(_) => Ok(MediaStreamType::None),
            None => Err(MimeError::ClassifierBackend {
                backend: Self::COMMAND.to_owned(),
                reason: "ffprobe exited without a numeric exit code".to_owned(),
            }),
        }
    }

    /// Creates the default command runner for FFprobe classification.
    ///
    /// Timeout comes from `config.command_timeout()`.
    ///
    /// # Parameters
    /// - `config`: MIME settings that supply the timeout and output limits.
    ///
    /// # Returns
    /// Runner used by the default classifier.
    fn default_command_runner(config: &MimeConfig) -> CommandRunner {
        CommandRunner::new(config.command_timeout())
            .disable_logging(true)
            .bounded_output(config.command_output_max_bytes())
            .max_total_output_bytes(config.command_total_output_max_bytes())
    }

    /// Builds the structured `ffprobe` command for one path.
    ///
    /// # Parameters
    /// - `path`: Local file path passed as an argument without shell parsing.
    ///
    /// # Returns
    /// Structured command description.
    fn command_for_path(path: &Path) -> Command {
        Command::new(Self::COMMAND)
            .arg("-v")
            .arg("error")
            .arg("-show_entries")
            .arg("stream=codec_type")
            .arg("-of")
            .arg("csv=p=0")
            .arg("-i")
            .sensitive_arg_os(path)
    }
}

impl Default for FfprobeCommandMediaStreamClassifier {
    /// Creates the default classifier.
    fn default() -> Self {
        Self::new()
    }
}

impl FileBasedMediaStreamClassifier for FfprobeCommandMediaStreamClassifier {
    /// Gets the maximum bytes staged from reader/content input.
    ///
    /// # Returns
    /// Maximum accepted stream size before staging is rejected.
    fn max_staging_size(&self) -> u64 {
        self.max_staging_size
    }

    /// Classifies a readable local media file using FFprobe.
    ///
    /// # Parameters
    /// - `file`: Readable local media file to inspect.
    ///
    /// # Returns
    /// Media stream classification.
    ///
    /// # Errors
    /// Returns [`MimeError::Command`](crate::MimeError::Command) when process
    /// execution itself fails.
    fn classify_by_local_file(&self, file: &Path) -> MimeResult<MediaStreamType> {
        self.classify_with_ffprobe(file)
    }
}

impl AsyncMediaStreamClassifierBackend for FfprobeCommandMediaStreamClassifier {
    fn classify_in<'a>(
        &'a self,
        scope: &'a qubit_operation::OperationScope,
        input: BorrowedMimeInput<'a>,
        options: &'a MimeOperationOptions,
    ) -> MimeFuture<'a, MediaStreamType> {
        Box::pin(async move {
            let services = self
                .execution_services
                .as_ref()
                .ok_or(MimeError::ExecutionServicesRequired)?;
            let read_tree = services.read_tree_for_operation(scope.id())?;
            let (owned_input, filename) = match input {
                BorrowedMimeInput::Bytes {
                    content,
                    extent,
                    filename,
                } => {
                    if extent != InputExtent::Complete {
                        return Err(MimeError::CompleteContentRequired);
                    }
                    (
                        OwnedMimeInput::bytes(Arc::from(content), extent),
                        filename.map(str::to_owned),
                    )
                }
                BorrowedMimeInput::Provider { file_system, path } => (
                    OwnedMimeInput::Provider {
                        file_system: Arc::new(file_system.clone()),
                        path: path.clone(),
                    },
                    path.file_name().map(str::to_owned),
                ),
            };
            let owned_input = if let Some(name) = filename {
                owned_input.with_filename(name)
            } else {
                owned_input
            };
            let max_staging_size = options
                .max_staging_bytes
                .unwrap_or(self.max_staging_size)
                .min(self.max_staging_size);
            let worker = MimeExecutionWorker::new(Arc::clone(&services.execution));
            let limits = ManagedStagingLimits {
                max_bytes: max_staging_size,
                budget: services.staging_budget(),
                worker: worker.clone(),
            };
            let staged = stage_owned_in(scope, owned_input, limits, read_tree).await?;
            let mut command = Self::command_for_path(&staged.path()?);
            if let Some(working_directory) = &self.working_directory {
                command = command.working_directory(working_directory);
            }
            let executor = ManagedCommandExecutor::new(worker);
            let output = executor
                .run_in(
                    scope,
                    ManagedMimeCommand::new(
                        self.command_runner.clone(),
                        command,
                        services.root_group.clone(),
                    ),
                    staged,
                )
                .await?;
            stream_type_from_output(&output)
        })
    }
}
