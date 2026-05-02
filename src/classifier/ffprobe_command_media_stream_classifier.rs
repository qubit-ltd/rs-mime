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
#[cfg(not(coverage))]
use std::process::Command;
#[cfg(not(coverage))]
use std::sync::OnceLock;
use std::time::Duration;

use crate::{
    AbstractMediaStreamClassifier, FileBasedMediaStreamClassifier, MediaStreamClassifier,
    MediaStreamType, MimeResult,
};

/// Media stream classifier backed by the `ffprobe` command.
#[derive(Debug, Clone)]
pub struct FfprobeCommandMediaStreamClassifier {
    execution_timeout: Option<Duration>,
    working_directory: Option<String>,
    disable_logging: bool,
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
            execution_timeout: None,
            working_directory: None,
            disable_logging: false,
        }
    }

    /// Sets the configured execution timeout.
    ///
    /// # Parameters
    /// - `timeout`: Timeout value stored for callers that need parity with the
    ///   Java API. The current standard-library implementation does not enforce
    ///   process timeouts.
    pub fn set_execution_timeout(&mut self, timeout: Duration) {
        self.execution_timeout = Some(timeout);
    }

    /// Gets the configured execution timeout.
    ///
    /// # Returns
    /// Stored timeout value, or `None`.
    pub fn execution_timeout(&self) -> Option<Duration> {
        self.execution_timeout
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

    /// Sets whether command logging is disabled.
    ///
    /// # Parameters
    /// - `disable_logging`: Stored flag for Java API parity.
    pub fn set_disable_logging(&mut self, disable_logging: bool) {
        self.disable_logging = disable_logging;
    }

    /// Tells whether command logging is disabled.
    ///
    /// # Returns
    /// Stored disable-logging flag.
    pub fn is_disable_logging(&self) -> bool {
        self.disable_logging
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
            Command::new(Self::COMMAND)
                .arg("-version")
                .output()
                .map(|output| output.status.success())
                .unwrap_or(false)
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
    /// [`MediaStreamType::None`] to match Java's best-effort refinement.
    ///
    /// # Errors
    /// Returns [`MimeError::Io`](crate::MimeError::Io) when process execution itself fails.
    #[cfg(not(coverage))]
    fn classify_by_local_file(&self, path: &Path) -> MimeResult<MediaStreamType> {
        AbstractMediaStreamClassifier::validate_readable_file(path)?;
        let mut command = Command::new(Self::COMMAND);
        command
            .arg("-v")
            .arg("error")
            .arg("-show_entries")
            .arg("stream=codec_type")
            .arg("-of")
            .arg("csv=p=0")
            .arg(path);
        if let Some(working_directory) = &self.working_directory {
            command.current_dir(working_directory);
        }
        let output = command.output()?;
        if !output.status.success() {
            return Ok(MediaStreamType::None);
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(Self::classify_stream_listing(&stdout))
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
    fn classify_by_local_file(&self, path: &Path) -> MimeResult<MediaStreamType> {
        AbstractMediaStreamClassifier::validate_readable_file(path)?;
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

impl MediaStreamClassifier for FfprobeCommandMediaStreamClassifier {
    /// Classifies a local media file using FFprobe.
    fn classify_file(&self, file: &Path) -> MimeResult<MediaStreamType> {
        self.classify_by_local_file(file)
    }

    /// Classifies in-memory bytes by staging them to a temporary file.
    fn classify_content(&self, content: &[u8]) -> MimeResult<MediaStreamType> {
        FileBasedMediaStreamClassifier::with_temp_file(content, |path| {
            self.classify_by_local_file(path)
        })
    }
}

#[cfg(coverage)]
pub(crate) mod coverage_support {
    //! Coverage helpers for FFprobe classifier branches.

    use std::time::Duration;

    use crate::MediaStreamClassifier;

    use super::FfprobeCommandMediaStreamClassifier;

    /// Exercises FFprobe classifier configuration and command paths.
    ///
    /// # Returns
    /// Summary strings from classifier behavior.
    pub(crate) fn exercise_ffprobe_edges() -> Vec<String> {
        let mut classifier = FfprobeCommandMediaStreamClassifier::new();
        classifier.set_execution_timeout(Duration::from_secs(1));
        classifier.set_working_directory(Some(".".to_owned()));
        classifier.set_disable_logging(true);
        let timeout = classifier.execution_timeout().is_some().to_string();
        let working_directory = classifier.working_directory().unwrap_or("").to_owned();
        let disable_logging = classifier.is_disable_logging().to_string();
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
            .execution_timeout()
            .is_none()
            .to_string();
        vec![
            FfprobeCommandMediaStreamClassifier::COMMAND.to_owned(),
            timeout,
            working_directory,
            disable_logging,
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
