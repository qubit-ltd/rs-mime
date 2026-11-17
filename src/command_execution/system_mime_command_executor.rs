// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Adapter from the managed runner to MIME-specific execution facts.
use qubit_command::Command;
use qubit_command::CommandCancellation;
use qubit_command::CommandError;
use qubit_command::CommandErrorKind;
use qubit_command::CommandRunOptions;
use qubit_command::CommandRunner;

use super::MimeCommandExecutor;
use super::MimeCommandOutput;

/// Production executor that delegates process ownership to CommandRunner.
pub(crate) struct SystemMimeCommandExecutor;
impl SystemMimeCommandExecutor {
    /// Runs with worker cancellation while retaining MIME's exit-code policy.
    ///
    /// # Parameters
    /// - `runner`: Managed runner responsible for command ownership.
    /// - `command`: Structured command to execute.
    /// - `cancellation`: Cancellation token for the worker.
    ///
    /// # Returns
    /// Captured command output, or the command execution error.
    pub(crate) fn run_cancellable(
        &self,
        runner: &CommandRunner,
        command: Command,
        cancellation: CommandCancellation,
    ) -> Result<MimeCommandOutput, CommandError> {
        Self::run_with_options(runner, command, CommandRunOptions::new().cancellation(cancellation))
    }

    /// Runs a command while applying the supplied runner options.
    ///
    /// # Parameters
    /// - `runner`: Managed runner responsible for command ownership.
    /// - `command`: Structured command to execute.
    /// - `options`: Cancellation and output policy for this execution.
    ///
    /// # Returns
    /// Captured output, including clean unexpected exits with output.
    ///
    /// # Errors
    /// Returns execution or cleanup failures that cannot be represented as
    /// command output.
    fn run_with_options(
        runner: &CommandRunner,
        command: Command,
        options: CommandRunOptions,
    ) -> Result<MimeCommandOutput, CommandError> {
        match runner.run_with(command, options) {
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

impl MimeCommandExecutor for SystemMimeCommandExecutor {
    /// Preserves execution errors and restores output only for clean unexpected
    /// exits.
    ///
    /// # Parameters
    /// - `runner`: Managed command runner that owns process execution.
    /// - `command`: Structured command to execute.
    ///
    /// # Returns
    /// Captured output for successful execution or a clean unexpected exit.
    ///
    /// # Errors
    /// Returns execution and cleanup failures that cannot be safely represented
    /// as command output.
    fn run(&self, runner: &CommandRunner, command: Command) -> Result<MimeCommandOutput, CommandError> {
        Self::run_with_options(runner, command, CommandRunOptions::new())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use qubit_command::Command;
    use qubit_command::CommandCancellation;
    use qubit_command::CommandErrorKind;
    use qubit_command::CommandRunner;

    use super::MimeCommandExecutor;
    use super::SystemMimeCommandExecutor;

    #[test]
    fn test_cancellable_executor_observes_pre_cancelled_token_before_spawn() {
        let cancellation = CommandCancellation::new();
        cancellation.cancel();
        let error = match SystemMimeCommandExecutor.run_cancellable(
            &CommandRunner::new(Duration::from_secs(1)).disable_logging(true),
            Command::new("must-not-start-after-cancellation"),
            cancellation,
        ) {
            Ok(_) => panic!("pre-cancelled execution should not start a process"),
            Err(error) => error,
        };
        assert_eq!(CommandErrorKind::CancelledBeforeStart, error.kind());
        assert!(error.output().is_none());
    }

    #[test]
    fn test_executor_preserves_output_for_clean_unexpected_exit() {
        let output = SystemMimeCommandExecutor
            .run(
                &CommandRunner::new(Duration::from_secs(5)).disable_logging(true),
                Command::new("rustc").arg("--qubit-mime-invalid-option"),
            )
            .expect("a clean unexpected exit retains the captured output");

        assert_ne!(Some(0), output.exit_code);
        assert!(output.stdout_complete);
    }
}
