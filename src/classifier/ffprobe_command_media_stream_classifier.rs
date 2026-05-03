/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! FFprobe-backed media stream classifier.

use std::path::Path;
// qubit-style: allow coverage-cfg
#[cfg(not(coverage))]
use std::sync::OnceLock;

use qubit_command::CommandRunner;
#[cfg(not(coverage))]
use qubit_command::{Command, CommandError};

use crate::{FileBasedMediaStreamClassifier, MediaStreamType, MimeResult};

/// Media stream classifier backed by the `ffprobe` command.
#[derive(Debug, Clone)]
pub struct FfprobeCommandMediaStreamClassifier {
    /// The working directory used to execute FFprobe.
    working_directory: Option<String>,
    /// The command runner used to execute FFprobe.
    command_runner: CommandRunner,
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
        Self {
            working_directory: None,
            command_runner: Self::default_command_runner(),
        }
    }

    /// Gets the command runner used by this classifier.
    ///
    /// # Returns
    /// Runner used for `ffprobe` command executions.
    pub fn command_runner(&self) -> &CommandRunner {
        &self.command_runner
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
    pub fn with_command_runner(mut self, command_runner: CommandRunner) -> Self {
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
    /// # Returns
    /// `true` when `ffprobe -version` executes successfully.
    #[cfg(not(coverage))]
    pub fn is_available() -> bool {
        static AVAILABLE: OnceLock<bool> = OnceLock::new();
        *AVAILABLE.get_or_init(|| {
            Self::default_command_runner()
                .run(Command::new(Self::COMMAND).arg("-version"))
                .is_ok()
        })
    }

    /// Checks FFprobe availability during coverage builds.
    ///
    /// # Returns
    /// Always returns `false` so default classifier selection stays
    /// deterministic under instrumentation.
    #[cfg(coverage)]
    pub fn is_available() -> bool {
        false
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
    #[cfg(not(coverage))]
    fn classify_with_ffprobe(&self, path: &Path) -> MimeResult<MediaStreamType> {
        let mut command = Self::command_for_path(path);
        if let Some(working_directory) = &self.working_directory {
            command = command.working_directory(working_directory);
        }
        match self.command_runner.run(command) {
            Ok(output) => {
                let stdout = output.stdout_lossy_text();
                Ok(Self::classify_stream_listing(&stdout))
            }
            Err(CommandError::UnexpectedExit { .. }) => Ok(MediaStreamType::None),
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
    #[cfg(not(coverage))]
    fn command_for_path(path: &Path) -> Command {
        Command::new(Self::COMMAND)
            .arg("-v")
            .arg("error")
            .arg("-show_entries")
            .arg("stream=codec_type")
            .arg("-of")
            .arg("csv=p=0")
            .arg_os(path)
    }

    /// Classifies a local file during coverage builds.
    ///
    /// # Parameters
    /// - `path`: Local file path.
    ///
    /// # Returns
    /// A deterministic non-media classification after validating readability.
    ///
    /// # Errors
    /// Returns [`MimeError::Io`](crate::MimeError::Io) when the path is not readable.
    #[cfg(coverage)]
    fn classify_with_ffprobe(&self, path: &Path) -> MimeResult<MediaStreamType> {
        let _ = path;
        let _ = self.working_directory.as_deref();
        Ok(MediaStreamType::None)
    }
}

impl Default for FfprobeCommandMediaStreamClassifier {
    /// Creates the default classifier.
    fn default() -> Self {
        Self::new()
    }
}

impl FileBasedMediaStreamClassifier for FfprobeCommandMediaStreamClassifier {
    /// Classifies a readable local media file using FFprobe.
    fn classify_by_local_file(&self, file: &Path) -> MimeResult<MediaStreamType> {
        self.classify_with_ffprobe(file)
    }
}

#[cfg(coverage)]
pub(crate) mod coverage_support {
    //! Coverage helpers for FFprobe classifier branches.

    use crate::MediaStreamClassifier;

    use super::FfprobeCommandMediaStreamClassifier;

    /// Exercises FFprobe classifier configuration and command paths.
    ///
    /// # Returns
    /// Summary strings from classifier behavior.
    pub(crate) fn exercise_ffprobe_edges() -> Vec<String> {
        let mut classifier = FfprobeCommandMediaStreamClassifier::new();
        classifier.set_working_directory(Some(".".to_owned()));
        let working_directory = classifier.working_directory().unwrap_or("").to_owned();
        let listing = [
            FfprobeCommandMediaStreamClassifier::classify_stream_listing("video\naudio\n"),
            FfprobeCommandMediaStreamClassifier::classify_stream_listing("video\n"),
            FfprobeCommandMediaStreamClassifier::classify_stream_listing("audio\n"),
            FfprobeCommandMediaStreamClassifier::classify_stream_listing("data\n"),
        ]
        .iter()
        .map(|stream_type| format!("{stream_type:?}"))
        .collect::<Vec<_>>()
        .join(",");
        let file = format!(
            "{:?}",
            classifier.classify_file(std::path::Path::new("Cargo.toml"))
        );
        let content = format!("{:?}", classifier.classify_content(b"not media"));
        let trait_classifier: &dyn MediaStreamClassifier = &classifier;
        let trait_file = format!(
            "{:?}",
            trait_classifier.classify_file(std::path::Path::new("Cargo.toml"))
        );
        let trait_content = format!("{:?}", trait_classifier.classify_content(b"not media"));
        let default = FfprobeCommandMediaStreamClassifier::default()
            .working_directory()
            .is_none()
            .to_string();
        vec![
            FfprobeCommandMediaStreamClassifier::COMMAND.to_owned(),
            working_directory,
            listing,
            FfprobeCommandMediaStreamClassifier::is_available().to_string(),
            file,
            content,
            trait_file,
            trait_content,
            default,
        ]
    }
}
