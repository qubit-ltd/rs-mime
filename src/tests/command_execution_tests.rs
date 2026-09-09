// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Tests the private command result boundary without shell or PATH mutation.
use std::time::Duration;

use qubit_command::Command;
use qubit_command::CommandErrorKind;
use qubit_command::CommandRunner;

use crate::command_execution::MimeCommandExecutor;
use crate::command_execution::MimeCommandOutput;
use crate::command_execution::SystemMimeCommandExecutor;
use crate::command_execution::require_complete_stdout;

#[test]
fn test_require_complete_stdout_rejects_unusable_streams() {
    for (bytes, truncated, complete, expected) in [
        (b"video\n".as_slice(), true, true, "stdout was truncated"),
        (b"video\n".as_slice(), false, false, "stdout is incomplete"),
        ([255].as_slice(), false, true, "stdout is not valid UTF-8"),
    ] {
        let output = MimeCommandOutput {
            exit_code: Some(0),
            stdout: bytes.to_vec(),
            stdout_truncated: truncated,
            stdout_complete: complete,
        };
        assert_eq!(require_complete_stdout(&output), Err(expected));
    }
    let output = MimeCommandOutput {
        exit_code: Some(0),
        stdout: b"video\n".to_vec(),
        stdout_truncated: false,
        stdout_complete: true,
    };
    assert_eq!(require_complete_stdout(&output), Ok("video\n"));
}

#[test]
fn test_executor_preserves_actual_status_and_strict_failures() {
    let runner = CommandRunner::new(Duration::from_secs(10)).disable_logging(true);
    let output = SystemMimeCommandExecutor
        .run(
            &runner.clone().success_exit_codes(&[7]),
            Command::new("rustc").arg("--version"),
        )
        .unwrap_or_else(|error| panic!("exit zero must survive runner policy: {error}"));
    assert_eq!(output.exit_code, Some(0));
    assert!(output.stdout.starts_with(b"rustc "));
    let output = SystemMimeCommandExecutor
        .run(
            &runner,
            Command::new("rustc").arg("--qubit-invalid-command-test-option"),
        )
        .unwrap_or_else(|error| panic!("numeric nonzero status must reach business policy: {error}"));
    assert!(output.exit_code.is_some_and(|code| code != 0));
    let error = match SystemMimeCommandExecutor.run(
        &runner.clone().max_output_bytes(1),
        Command::new("rustc").arg("--version"),
    ) {
        Ok(_) => panic!("strict output limit must fail"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), CommandErrorKind::OutputTruncated);
    let error = match SystemMimeCommandExecutor.run(&runner, Command::new("__qubit_mime_missing_test_executable__")) {
        Ok(_) => panic!("missing program must fail"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), CommandErrorKind::SpawnFailed);
}
