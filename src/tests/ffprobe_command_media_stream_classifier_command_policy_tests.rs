// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

//! Tests FFprobe command policy using controlled command output.

use std::path::Path;

use qubit_command::Command;
use qubit_command::CommandError;
use qubit_command::CommandRunner;

use crate::FfprobeCommandMediaStreamClassifier;
use crate::MediaStreamType;
use crate::MimeConfig;
use crate::command_execution::MimeCommandExecutor;
use crate::command_execution::MimeCommandOutput;

struct FixedOutputExecutor {
    exit_code: Option<i32>,
    stdout: Vec<u8>,
    truncated: bool,
    complete: bool,
}

impl MimeCommandExecutor for FixedOutputExecutor {
    fn run(&self, _runner: &CommandRunner, _command: Command) -> Result<MimeCommandOutput, CommandError> {
        Ok(MimeCommandOutput {
            exit_code: self.exit_code,
            stdout: self.stdout.clone(),
            stdout_truncated: self.truncated,
            stdout_complete: self.complete,
        })
    }
}

fn executor(exit_code: Option<i32>, stdout: &[u8], truncated: bool, complete: bool) -> FixedOutputExecutor {
    FixedOutputExecutor {
        exit_code,
        stdout: stdout.to_vec(),
        truncated,
        complete,
    }
}

#[test]
fn test_local_classifier_maps_status_and_complete_stdout_policy() {
    let mut classifier = FfprobeCommandMediaStreamClassifier::from_mime_config(MimeConfig::default());
    classifier.set_working_directory(Some("/recorded/working-dir".to_owned()));

    let audio = classifier
        .classify_with_executor(
            Path::new("relative-media-file"),
            &executor(Some(0), b"audio\n", false, true),
        )
        .unwrap();
    assert_eq!(MediaStreamType::AudioOnly, audio);

    let nonzero = classifier
        .classify_with_executor(
            Path::new("relative-media-file"),
            &executor(Some(1), b"video\n", false, true),
        )
        .unwrap();
    assert_eq!(MediaStreamType::None, nonzero);

    let signal_exit = classifier
        .classify_with_executor(Path::new("relative-media-file"), &executor(None, b"", false, true))
        .expect_err("termination without a numeric status is a backend error");
    assert!(
        matches!(signal_exit, crate::MimeError::ClassifierBackend { reason, .. } if reason.contains("numeric exit code"))
    );

    let truncated = classifier
        .classify_with_executor(
            Path::new("relative-media-file"),
            &executor(Some(0), b"video\n", true, true),
        )
        .expect_err("truncated stream listings cannot be classified");
    assert!(matches!(truncated, crate::MimeError::ClassifierBackend { reason, .. } if reason.contains("truncated")));
}

#[test]
fn test_availability_requires_zero_exit_and_complete_output() {
    let config = MimeConfig::default();
    assert!(FfprobeCommandMediaStreamClassifier::is_available_with_executor(
        &config,
        &executor(Some(0), b"ffprobe version", false, true),
    ));
    assert!(!FfprobeCommandMediaStreamClassifier::is_available_with_executor(
        &config,
        &executor(Some(1), b"ffprobe version", false, true),
    ));
    assert!(!FfprobeCommandMediaStreamClassifier::is_available_with_executor(
        &config,
        &executor(Some(0), b"ffprobe version", false, false),
    ));
}
