// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Minimal execution facts required by MIME business policy.
use qubit_command::CommandOutput;

/// Owned stdout and actual exit status, independent of runner success policy.
pub(crate) struct MimeCommandOutput {
    /// Numeric exit status, or None for termination without an exit code.
    pub(crate) exit_code: Option<i32>,
    /// Raw captured stdout; deliberately omitted from Debug output.
    pub(crate) stdout: Vec<u8>,
    /// Whether the in-memory stdout capture exceeded its limit.
    pub(crate) stdout_truncated: bool,
    /// Whether stdout reached EOF before cancellation.
    pub(crate) stdout_complete: bool,
}
impl MimeCommandOutput {
    /// Moves command output into the private projection without copying stdout.
    pub(super) fn from_output(output: CommandOutput) -> Self {
        let exit_code = output.exit_code();
        let stdout_truncated = output.stdout_truncated();
        let stdout_complete = output.stdout_complete();
        Self {
            exit_code,
            stdout: output.into_stdout(),
            stdout_truncated,
            stdout_complete,
        }
    }
}

/// Borrows complete UTF-8 stdout or returns a safe description of unusable
/// data.
///
/// Truncated, incomplete and invalid UTF-8 output is rejected without exposing
/// captured bytes in diagnostic messages.
pub(crate) fn require_complete_stdout(output: &MimeCommandOutput) -> Result<&str, &'static str> {
    if output.stdout_truncated {
        return Err("stdout was truncated");
    }
    if !output.stdout_complete {
        return Err("stdout is incomplete");
    }
    std::str::from_utf8(&output.stdout).map_err(|_| "stdout is not valid UTF-8")
}
