// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! A structured command paired with its runner configuration.

use qubit_command::{Command, CommandRunner};
use qubit_operation::OperationGroup;

/// Command request handed to the MIME execution worker.
#[derive(Clone)]
pub(crate) struct ManagedMimeCommand {
    pub(crate) runner: CommandRunner,
    pub(crate) command: Command,
    pub(crate) runtime_group: OperationGroup,
}

impl ManagedMimeCommand {
    /// Creates a managed request without flattening argv or runner settings.
    pub(crate) fn new(
        runner: CommandRunner,
        command: Command,
        runtime_group: OperationGroup,
    ) -> Self {
        Self {
            runner,
            command,
            runtime_group,
        }
    }
}
