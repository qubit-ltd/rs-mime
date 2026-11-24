// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Rejection reasons reported by MIME execution ports.

/// Reason a command or I/O job could not enter the execution queue.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum MimeExecutionDispatchFailureKind {
    /// The prepared queue is open but has no free queued-job slot.
    #[error("MIME execution queue is full")]
    QueueFull,
    /// The execution service is stopped, closed, or not prepared.
    #[error("MIME execution service is unavailable")]
    Unavailable,
}
