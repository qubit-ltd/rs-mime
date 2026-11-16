// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Private execution boundary shared by command-backed MIME components.
mod async_mime_command_executor;
mod managed_mime_command;
mod mime_command_executor;
mod mime_command_output;
mod mime_execution_worker;
mod system_mime_command_executor;
pub(crate) use async_mime_command_executor::AsyncMimeCommandExecutor;
pub(crate) use async_mime_command_executor::ManagedCommandExecutor;
pub(crate) use managed_mime_command::ManagedMimeCommand;
pub(crate) use mime_command_executor::MimeCommandExecutor;
pub(crate) use mime_command_output::MimeCommandOutput;
pub(crate) use mime_command_output::require_complete_stdout;
pub use mime_execution_worker::FixedMimeExecutionPort;
pub(crate) use mime_execution_worker::MimeExecutionWorker;
pub(crate) use system_mime_command_executor::SystemMimeCommandExecutor;
