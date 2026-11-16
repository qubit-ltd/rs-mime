// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Private lifecycle state and polling signals for managed command execution.

mod poll_signal;
mod prepared_command;
mod signal_state;

pub(super) use poll_signal::PollSignal;
pub(super) use prepared_command::PreparedCommand;
pub(super) use signal_state::SignalState;
