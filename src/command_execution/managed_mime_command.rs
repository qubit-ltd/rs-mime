// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! A structured command paired with its runner configuration.

use qubit_command::Command;
use qubit_command::CommandRunner;
use qubit_operation::OperationGroup;

/// Command request handed to the MIME execution worker.
#[derive(Clone)]
pub(crate) struct ManagedMimeCommand {
    /// Runner limits and cancellation policy for this command.
    pub(crate) runner: CommandRunner,
    /// Structured executable and arguments; no shell string is constructed.
    pub(crate) command: Command,
    /// Operation group that accepts the command child.
    pub(crate) runtime_group: OperationGroup,
}

impl ManagedMimeCommand {
    /// Creates a managed request without flattening argv or runner settings.
    ///
    /// # Parameters
    /// - `runner`: Command execution policy and output limits.
    /// - `command`: Executable and structured arguments to run.
    /// - `runtime_group`: Group used when submitting the operation child.
    ///
    /// # Returns
    /// A request retaining all command and operation settings.
    #[must_use]
    pub(crate) fn new(runner: CommandRunner, command: Command, runtime_group: OperationGroup) -> Self {
        Self {
            runner,
            command,
            runtime_group,
        }
    }
}
