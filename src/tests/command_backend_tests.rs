// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Exercises MIME policy using execution facts without modifying PATH.
use std::ffi::OsStr;
use std::path::Path;
use std::time::Duration;

use qubit_command::Command;
use qubit_command::CommandError;
use qubit_command::CommandErrorKind;
use qubit_command::CommandRunner;

use crate::FfprobeCommandMediaStreamClassifier;
use crate::FileCommandMimeDetector;
use crate::MediaStreamType;
use crate::MimeError;
use crate::command_execution::MimeCommandExecutor;
use crate::command_execution::MimeCommandOutput;

/// Returns specified facts and checks that each backend preserves structured
/// argv.
struct FakeExecutor {
    code: Option<i32>,
    bytes: Vec<u8>,
    truncated: bool,
    complete: bool,
}
impl MimeCommandExecutor for FakeExecutor {
    fn run(&self, _: &CommandRunner, command: Command) -> Result<MimeCommandOutput, CommandError> {
        let args: Vec<_> = command.arguments().collect();
        assert_eq!(args.last().copied(), Some(OsStr::new("-private-media")));
        let separator = if command.program() == "file" { "--" } else { "-i" };
        assert_eq!(args[args.len() - 2], separator);
        Ok(MimeCommandOutput {
            exit_code: self.code,
            stdout: self.bytes.clone(),
            stdout_truncated: self.truncated,
            stdout_complete: self.complete,
        })
    }
}

#[test]
fn test_command_backend_policy_matrix() {
    let path = Path::new("-private-media");
    let runner = CommandRunner::new(Duration::from_secs(10))
        .fail_on_output_truncation(false)
        .success_exit_codes(&[7]);
    let ffprobe = FfprobeCommandMediaStreamClassifier::new().with_command_runner(runner.clone());
    let file = FileCommandMimeDetector::default().with_command_runner(runner);
    for (code, bytes, truncated, complete, classification, file_success) in [
        (
            Some(0),
            b"video\naudio\n".as_slice(),
            false,
            true,
            Some(MediaStreamType::VideoWithAudio),
            true,
        ),
        (Some(0), b"video\n".as_slice(), true, true, None, false),
        (Some(0), b"video\n".as_slice(), false, false, None, false),
        (Some(0), [255].as_slice(), false, true, None, false),
        (
            Some(7),
            b"video\n".as_slice(),
            false,
            true,
            Some(MediaStreamType::None),
            false,
        ),
        (
            Some(7),
            [255].as_slice(),
            true,
            false,
            Some(MediaStreamType::None),
            false,
        ),
        (None, b"video\n".as_slice(), false, true, None, false),
        (Some(0), b"".as_slice(), false, true, Some(MediaStreamType::None), true),
    ] {
        let executor = FakeExecutor {
            code,
            bytes: bytes.to_vec(),
            truncated,
            complete,
        };
        let result = ffprobe.classify_with_executor(path, &executor);
        match classification {
            Some(expected) => assert_eq!(result.expect("valid facts must classify"), expected),
            None => assert!(
                matches!(result, Err(MimeError::ClassifierBackend { ref backend, ref reason }) if backend == "ffprobe" && !reason.contains("private-media"))
            ),
        }
        let result = file.guess_with_executor(path, &executor);
        if file_success {
            let expected = if bytes.is_empty() {
                Vec::new()
            } else {
                vec![std::str::from_utf8(bytes).expect("valid UTF-8 case").trim().to_owned()]
            };
            assert_eq!(result.expect("complete success must parse"), expected);
        } else {
            assert!(
                matches!(result, Err(MimeError::DetectorBackend { ref backend, ref reason }) if backend == "file" && !reason.contains("private-media"))
            );
        }
    }
}

/// Produces a real spawn failure without exposing any CommandError constructor.
struct FailingExecutor;
impl MimeCommandExecutor for FailingExecutor {
    fn run(&self, runner: &CommandRunner, _: Command) -> Result<MimeCommandOutput, CommandError> {
        Err(runner
            .run(Command::new("__qubit_mime_missing_backend_test__"))
            .expect_err("missing executable must fail"))
    }
}

#[test]
fn test_command_backends_propagate_execution_failure() {
    let path = Path::new("-private-media");
    let ffprobe = FfprobeCommandMediaStreamClassifier::new().classify_with_executor(path, &FailingExecutor);
    assert!(matches!(ffprobe, Err(MimeError::Command(ref error)) if error.kind() == CommandErrorKind::SpawnFailed));
    let file = FileCommandMimeDetector::default().guess_with_executor(path, &FailingExecutor);
    assert!(matches!(file, Err(MimeError::Command(ref error)) if error.kind() == CommandErrorKind::SpawnFailed));
}
