// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Private execution interface used by both MIME command backends.
use qubit_command::Command;
use qubit_command::CommandError;
use qubit_command::CommandRunner;

use super::MimeCommandOutput;

/// Runs a structured command while preserving failures unrelated to exit
/// policy.
pub(crate) trait MimeCommandExecutor {
    /// Executes synchronously using the supplied runner configuration.
    ///
    /// Returns actual status and stdout, including clean unexpected exits.
    /// Preparation, process, I/O, timeout, cancellation and truncation failures
    /// remain CommandError values instead of becoming business successes.
    ///
    /// # Parameters
    /// - `runner`: Managed command runner that owns process execution.
    /// - `command`: Structured command to execute.
    ///
    /// # Returns
    /// Captured MIME command output with the actual exit status.
    ///
    /// # Errors
    /// Returns preparation, process, I/O, timeout, cancellation, or truncation
    /// failures as `CommandError` values.
    fn run(&self, runner: &CommandRunner, command: Command) -> Result<MimeCommandOutput, CommandError>;
}
