// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Private execution boundary shared by command-backed MIME components.
mod mime_command_executor;
mod mime_command_output;
mod system_mime_command_executor;
pub(crate) use mime_command_executor::MimeCommandExecutor;
pub(crate) use mime_command_output::MimeCommandOutput;
pub(crate) use mime_command_output::require_complete_stdout;
pub(crate) use system_mime_command_executor::SystemMimeCommandExecutor;
