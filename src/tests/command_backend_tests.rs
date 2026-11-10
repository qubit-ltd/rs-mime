// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Exercises MIME policy using execution facts without modifying PATH.
use std::path::Path;
use std::time::Duration;

use qubit_command::Command;
use qubit_command::CommandError;
use qubit_command::CommandErrorKind;
use qubit_command::CommandRunner;
use qubit_config::Config;

use crate::CONFIG_COMMAND_OUTPUT_MAX_BYTES;
use crate::FfprobeCommandMediaStreamClassifier;
use crate::FileCommandMimeDetector;
use crate::MediaStreamType;
use crate::MimeConfig;
use crate::MimeError;
use crate::command_execution::MimeCommandExecutor;
use crate::command_execution::MimeCommandOutput;

#[test]
fn test_default_command_backends_configure_derived_total_output_budget() {
    let config = MimeConfig::default();
    let expected = config.command_total_output_max_bytes();
    let ffprobe = FfprobeCommandMediaStreamClassifier::from_mime_config(config.clone());
    let file = FileCommandMimeDetector::from_mime_config(config);
    assert_eq!(
        ffprobe.command_runner().configured_max_total_output_bytes(),
        Some(expected)
    );
    assert_eq!(
        file.command_runner().configured_max_total_output_bytes(),
        Some(expected)
    );

    let mut zero_config = Config::new();
    zero_config
        .set(CONFIG_COMMAND_OUTPUT_MAX_BYTES, 0_u64)
        .expect("zero output limit should be accepted");
    let zero = MimeConfig::from_config(&zero_config).expect("zero output limit should parse");
    assert_eq!(zero.command_total_output_max_bytes(), 0);

    let mut maximum_config = Config::new();
    maximum_config
        .set(CONFIG_COMMAND_OUTPUT_MAX_BYTES, usize::MAX as u64)
        .expect("platform maximum output limit should be accepted");
    let maximum = MimeConfig::from_config(&maximum_config).expect("platform maximum should parse");
    assert_eq!(
        maximum.command_total_output_max_bytes(),
        (usize::MAX as u64).saturating_mul(4)
    );
}

#[test]
fn test_injected_command_backend_runner_keeps_its_total_output_policy() {
    let without_limit = CommandRunner::new(Duration::from_secs(3)).without_total_output_limit();
    let ffprobe = FfprobeCommandMediaStreamClassifier::new().with_command_runner(without_limit.clone());
    let file = FileCommandMimeDetector::default().with_command_runner(without_limit);
    assert_eq!(ffprobe.command_runner().configured_max_total_output_bytes(), None);
    assert_eq!(file.command_runner().configured_max_total_output_bytes(), None);

    let explicit_budget = CommandRunner::new(Duration::from_secs(3)).max_total_output_bytes(u64::MAX);
    let ffprobe = FfprobeCommandMediaStreamClassifier::new().with_command_runner(explicit_budget.clone());
    let file = FileCommandMimeDetector::default().with_command_runner(explicit_budget);
    assert_eq!(
        ffprobe.command_runner().configured_max_total_output_bytes(),
        Some(u64::MAX)
    );
    assert_eq!(
        file.command_runner().configured_max_total_output_bytes(),
        Some(u64::MAX)
    );
}

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
        let expected_path = std::path::absolute(Path::new("-private-media")).expect("cwd should be available");
        assert_eq!(args.last().copied(), Some(expected_path.as_os_str()));
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
            false,
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

#[test]
fn test_file_backend_accepts_only_one_mime_name_or_empty_output() {
    let path = Path::new("-private-media");
    let file = FileCommandMimeDetector::default();
    for (bytes, expected) in [
        (b"application/pdf\n".as_slice(), Some("application/pdf")),
        (
            b"application/x-custom+json".as_slice(),
            Some("application/x-custom+json"),
        ),
        (b"inode/directory".as_slice(), Some("inode/directory")),
        (b"".as_slice(), None),
    ] {
        let executor = FakeExecutor {
            code: Some(0),
            bytes: bytes.to_vec(),
            truncated: false,
            complete: true,
        };
        assert_eq!(
            expected.map(str::to_owned).into_iter().collect::<Vec<_>>(),
            file.guess_with_executor(path, &executor)
                .expect("valid MIME output should parse")
        );
    }
    for bytes in [
        b"cannot open private-media".as_slice(),
        b"video".as_slice(),
        b"video\naudio\n".as_slice(),
        b"text/plain; charset=utf-8".as_slice(),
        b"/plain".as_slice(),
        b"text/".as_slice(),
        b"text/plain/extra".as_slice(),
        "text/中文".as_bytes(),
    ] {
        let executor = FakeExecutor {
            code: Some(0),
            bytes: bytes.to_vec(),
            truncated: false,
            complete: true,
        };
        let error = file
            .guess_with_executor(path, &executor)
            .expect_err("invalid MIME output must fail");
        assert!(matches!(error, MimeError::DetectorBackend { ref backend, ref reason }
            if backend == "file" && reason == "file returned an invalid MIME type"
                && !reason.contains("private-media") && !reason.contains(std::str::from_utf8(bytes).unwrap_or(""))));
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

struct ProbeBudgetExecutor {
    timeout: Duration,
    output_limit: u64,
    total_output_limit: u64,
    command: &'static str,
    exit_code: Option<i32>,
    stdout_truncated: bool,
    stdout_complete: bool,
}

struct PathAssertionExecutor {
    expected_path: std::path::PathBuf,
}

impl MimeCommandExecutor for PathAssertionExecutor {
    fn run(&self, _: &CommandRunner, command: Command) -> Result<MimeCommandOutput, CommandError> {
        assert_eq!(
            command.arguments().last(),
            Some(self.expected_path.as_os_str()),
            "command input should be fixed to its process-cwd absolute path"
        );
        let stdout: &[u8] = if command.program() == "file" {
            b"video/mp4\n"
        } else {
            b"video\n"
        };
        Ok(MimeCommandOutput {
            exit_code: Some(0),
            stdout: stdout.to_vec(),
            stdout_truncated: false,
            stdout_complete: true,
        })
    }
}

#[test]
fn test_command_boundaries_absolute_paths_with_special_names_and_working_directory() {
    let ffprobe = FfprobeCommandMediaStreamClassifier::new();
    let repository = crate::repository::MimeRepository::empty();
    let file = FileCommandMimeDetector::with_repository_and_runner(
        &repository,
        CommandRunner::new(Duration::from_secs(1)).working_directory("child-working-directory"),
    );

    for path in [
        Path::new("-"),
        Path::new("目录 with space/media.bin"),
        Path::new("first/media.bin"),
        Path::new("second/media.bin"),
    ] {
        let expected_path = std::path::absolute(path).expect("cwd should be available");
        let executor = PathAssertionExecutor {
            expected_path: expected_path.clone(),
        };
        assert_eq!(
            MediaStreamType::VideoOnly,
            ffprobe
                .classify_with_executor(path, &executor)
                .expect("ffprobe command should preserve the absolute input path")
        );
        assert_eq!(
            vec!["video/mp4"],
            file.guess_with_executor(path, &executor)
                .expect("file command should preserve the absolute input path")
        );
    }
}

impl MimeCommandExecutor for ProbeBudgetExecutor {
    fn run(&self, runner: &CommandRunner, command: Command) -> Result<MimeCommandOutput, CommandError> {
        assert_eq!(Some(self.timeout), runner.configured_timeout());
        assert_eq!(Some(self.output_limit as usize), runner.configured_max_stdout_bytes());
        assert_eq!(Some(self.output_limit as usize), runner.configured_max_stderr_bytes());
        assert_eq!(
            Some(self.total_output_limit),
            runner.configured_max_total_output_bytes()
        );
        assert!(runner.is_logging_disabled());
        assert_eq!(self.command, command.program());
        Ok(MimeCommandOutput {
            exit_code: self.exit_code,
            stdout: b"available\n".to_vec(),
            stdout_truncated: self.stdout_truncated,
            stdout_complete: self.stdout_complete,
        })
    }
}

#[test]
fn test_availability_probes_use_supplied_timeout_and_output_budgets() {
    let timeout = Duration::from_millis(17);
    let output_limit = 3_u64;
    let mut source = Config::new();
    source
        .set(crate::CONFIG_COMMAND_TIMEOUT, "17ms")
        .expect("custom timeout should be accepted");
    source
        .set(CONFIG_COMMAND_OUTPUT_MAX_BYTES, output_limit)
        .expect("custom output limit should be accepted");
    let config = MimeConfig::from_config(&source).expect("custom config should parse");
    let executor = |command, exit_code, stdout_truncated, stdout_complete| ProbeBudgetExecutor {
        timeout,
        output_limit,
        total_output_limit: config.command_total_output_max_bytes(),
        command,
        exit_code,
        stdout_truncated,
        stdout_complete,
    };

    for (exit_code, truncated, complete, expected) in [
        (Some(0), false, true, true),
        (Some(7), false, true, false),
        (Some(0), true, true, false),
        (Some(0), false, false, false),
        (None, false, true, false),
    ] {
        assert_eq!(
            expected,
            FfprobeCommandMediaStreamClassifier::is_available_with_executor(
                &config,
                &executor("ffprobe", exit_code, truncated, complete)
            )
        );
        assert_eq!(
            expected,
            FileCommandMimeDetector::is_available_with_executor(
                &config,
                &executor("file", exit_code, truncated, complete)
            )
        );
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
