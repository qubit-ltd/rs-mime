// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Lifecycle state retained while a guarded worker job is queued.

/// Tracks the lifecycle of one guarded job before its payload is consumed.
///
/// # Type Parameters
/// - `T`: Payload transferred to the worker.
/// - `Run`: Callback executed when the worker starts.
/// - `Abandon`: Callback used when an accepted job is discarded first.
pub(super) struct GuardState<T, Run, Abandon> {
    /// Whether the execution port accepted the job.
    pub(super) accepted: bool,
    /// Whether the execution port returned the job synchronously.
    pub(super) rejected: bool,
    /// Whether the worker consumed the payload.
    pub(super) started: bool,
    /// Whether the queued closure was dropped.
    pub(super) dropped: bool,
    /// Payload retained until execution or abandonment.
    pub(super) payload: Option<T>,
    /// Execution callback retained until the worker starts.
    pub(super) run: Option<Run>,
    /// Terminal callback retained until abandonment is confirmed.
    pub(super) abandon: Option<Abandon>,
}
