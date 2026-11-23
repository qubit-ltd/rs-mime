// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Rejected execution jobs returned with their dispatch reason.

use std::fmt;

use super::mime_execution_dispatch_failure_kind::MimeExecutionDispatchFailureKind;

/// Rejected execution job and its dispatch reason.
#[must_use = "recover or explicitly drop the rejected execution job"]
pub struct MimeExecutionDispatchFailure {
    /// Queue saturation or service availability reason for this failure.
    kind: MimeExecutionDispatchFailureKind,
    /// Original owned job retained for caller recovery or retry.
    job: Box<dyn FnOnce() + Send + 'static>,
}

impl MimeExecutionDispatchFailure {
    /// Creates a failure that retains the rejected job for recovery or retry.
    ///
    /// # Parameters
    /// - `kind`: Reason the execution port rejected the job.
    /// - `job`: Original owned job that was not dispatched.
    ///
    /// # Returns
    /// A failure carrying both the reason and the original job.
    pub(crate) fn new(kind: MimeExecutionDispatchFailureKind, job: Box<dyn FnOnce() + Send + 'static>) -> Self {
        Self { kind, job }
    }

    /// Returns why dispatch was rejected.
    ///
    /// # Returns
    /// The queue saturation or service availability reason.
    #[must_use]
    pub fn kind(&self) -> MimeExecutionDispatchFailureKind {
        self.kind
    }

    /// Returns the original job so the caller can recover or retry it.
    ///
    /// # Returns
    /// The owned job that the execution port did not accept.
    #[must_use]
    pub fn into_job(self) -> Box<dyn FnOnce() + Send + 'static> {
        self.job
    }
}

impl fmt::Debug for MimeExecutionDispatchFailure {
    /// Formats the rejection reason without exposing the captured job.
    ///
    /// # Parameters
    /// - `f`: Formatter that receives the debug representation.
    ///
    /// # Returns
    /// The formatter result.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MimeExecutionDispatchFailure")
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}
