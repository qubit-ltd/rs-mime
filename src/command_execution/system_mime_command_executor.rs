// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Adapter from the managed runner to MIME-specific execution facts.
use qubit_command::Command;
use qubit_command::CommandError;
use qubit_command::CommandErrorKind;
use qubit_command::CommandRunner;

use super::MimeCommandExecutor;
use super::MimeCommandOutput;

/// Production executor that delegates process ownership to CommandRunner.
pub(crate) struct SystemMimeCommandExecutor;
impl MimeCommandExecutor for SystemMimeCommandExecutor {
    /// Preserves execution errors and restores output only for clean unexpected
    /// exits.
    fn run(&self, runner: &CommandRunner, command: Command) -> Result<MimeCommandOutput, CommandError> {
        match runner.run(command) {
            Ok(output) => Ok(MimeCommandOutput::from_output(output)),
            Err(error)
                if error.kind() == CommandErrorKind::UnexpectedExit
                    && error.cleanup_failures().is_empty()
                    && error.output().is_some() =>
            {
                Ok(MimeCommandOutput::from_output(
                    error.into_output().expect("checked command output must exist"),
                ))
            }
            Err(error) => Err(error),
        }
    }
}
