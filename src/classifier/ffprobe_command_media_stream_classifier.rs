// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! FFprobe-backed media stream classifier.

use std::path::Path;

use qubit_command::{
    Command,
    CommandError,
    CommandRunner,
};

use crate::{
    FileBasedMediaStreamClassifier,
    MediaStreamType,
    MimeConfig,
    MimeResult,
};

/// Media stream classifier backed by the `ffprobe` command.
#[derive(Debug, Clone)]
pub struct FfprobeCommandMediaStreamClassifier {
    /// The working directory used to execute FFprobe.
    working_directory: Option<String>,
    /// The command runner used to execute FFprobe.
    command_runner: CommandRunner,
    /// Maximum bytes staged from reader/content input.
    max_staging_size: u64,
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
    pub fn from_mime_config(config: MimeConfig) -> Self {
        Self {
            working_directory: None,
            command_runner: Self::default_command_runner(),
            max_staging_size: config.media_stream_max_staging_size(),
        }
    }

    /// Gets the command runner used by this classifier.
    ///
    /// # Returns
    /// Runner used for `ffprobe` command executions.
    pub fn command_runner(&self) -> &CommandRunner {
        &self.command_runner
    }

    /// Gets the maximum bytes staged from reader/content input.
    ///
    /// # Returns
    /// Maximum accepted stream size before staging is rejected.
    pub fn max_staging_size(&self) -> u64 {
        self.max_staging_size
    }

    /// Replaces the maximum bytes staged from reader/content input.
    ///
    /// # Parameters
    /// - `max_staging_size`: Maximum accepted stream size.
    pub fn set_max_staging_size(&mut self, max_staging_size: u64) {
        self.max_staging_size = max_staging_size;
    }

    /// Replaces the maximum staging size and returns the updated classifier.
    ///
    /// # Parameters
    /// - `max_staging_size`: Maximum accepted stream size.
    ///
    /// # Returns
    /// The updated classifier.
    pub fn with_max_staging_size(mut self, max_staging_size: u64) -> Self {
        self.max_staging_size = max_staging_size;
        self
    }

    /// Replaces the command runner used by this classifier.
    ///
    /// # Parameters
    /// - `command_runner`: New runner configuration.
    pub fn set_command_runner(&mut self, command_runner: CommandRunner) {
        self.command_runner = command_runner;
    }

    /// Replaces the command runner and returns the updated classifier.
    ///
    /// # Parameters
    /// - `command_runner`: New runner configuration.
    ///
    /// # Returns
    /// The updated classifier.
    pub fn with_command_runner(
        mut self,
        command_runner: CommandRunner,
    ) -> Self {
        self.command_runner = command_runner;
        self
    }

    /// Sets the working directory used to execute FFprobe.
    ///
    /// # Parameters
    /// - `working_directory`: Optional working directory path.
    pub fn set_working_directory(&mut self, working_directory: Option<String>) {
        self.working_directory = working_directory;
    }

    /// Gets the configured working directory.
    ///
    /// # Returns
    /// Stored working directory, or `None`.
    pub fn working_directory(&self) -> Option<&str> {
        self.working_directory.as_deref()
    }

    /// Classifies FFprobe `codec_type` output.
    ///
    /// # Parameters
    /// - `output`: Lines printed by `ffprobe -show_entries stream=codec_type`.
    ///
    /// # Returns
    /// Media stream classification.
    pub fn classify_stream_listing(output: &str) -> MediaStreamType {
        let has_video =
            output.lines().any(|line| line.trim() == Self::VIDEO_STREAM);
        let has_audio =
            output.lines().any(|line| line.trim() == Self::AUDIO_STREAM);
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
    pub fn is_available() -> bool {
        Self::default_command_runner()
            .run(Command::new(Self::COMMAND).arg("-version"))
            .is_ok()
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
    fn classify_with_ffprobe(
        &self,
        path: &Path,
    ) -> MimeResult<MediaStreamType> {
        let mut command = Self::command_for_path(path);
        if let Some(working_directory) = &self.working_directory {
            command = command.working_directory(working_directory);
        }
        match self.command_runner.run(command) {
            Ok(output) => {
                let stdout = output.stdout_lossy_text();
                Ok(Self::classify_stream_listing(&stdout))
            }
            Err(CommandError::UnexpectedExit { .. }) => {
                Ok(MediaStreamType::None)
            }
            Err(error) => Err(error.into()),
        }
    }

    /// Creates the default command runner for FFprobe classification.
    ///
    /// # Returns
    /// Runner used by the default classifier.
    fn default_command_runner() -> CommandRunner {
        CommandRunner::new().disable_logging(true)
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
    fn max_staging_size(&self) -> u64 {
        self.max_staging_size
    }

    /// Classifies a readable local media file using FFprobe.
    fn classify_by_local_file(
        &self,
        file: &Path,
    ) -> MimeResult<MediaStreamType> {
        self.classify_with_ffprobe(file)
    }
}
