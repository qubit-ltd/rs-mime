// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! MIME-owned command and I/O execution boundary.

use std::fmt;
use std::fmt::Debug;

use crate::MimeError;

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

/// Rejected execution job and its dispatch reason.
#[must_use = "recover or explicitly drop the rejected execution job"]
pub struct MimeExecutionDispatchFailure {
    kind: MimeExecutionDispatchFailureKind,
    job: Box<dyn FnOnce() + Send + 'static>,
}

impl MimeExecutionDispatchFailure {
    pub(crate) fn new(kind: MimeExecutionDispatchFailureKind, job: Box<dyn FnOnce() + Send + 'static>) -> Self {
        Self { kind, job }
    }

    /// Returns why dispatch was rejected.
    #[must_use]
    pub fn kind(&self) -> MimeExecutionDispatchFailureKind {
        self.kind
    }

    /// Returns the original job so the caller can recover or retry it.
    #[must_use]
    pub fn into_job(self) -> Box<dyn FnOnce() + Send + 'static> {
        self.job
    }
}

impl fmt::Debug for MimeExecutionDispatchFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MimeExecutionDispatchFailure")
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}

/// MIME-specific command and I/O execution boundary. Implementations are
/// MIME-owned adapters; their callbacks must preserve typed domain errors.
///
/// # Examples
///
/// Implement the lifecycle hooks and retain a rejected job for retry or
/// fallback when the default dispatcher declines it.
///
/// ```
/// use qubit_mime::{MimeError, MimeExecutionPort};
///
/// #[derive(Debug)]
/// struct LocalPort;
///
/// impl MimeExecutionPort for LocalPort {
///     fn prepare(&self) -> Result<(), MimeError> { Ok(()) }
///     fn shutdown(&self) -> Result<(), MimeError> { Ok(()) }
/// }
///
/// let port = LocalPort;
/// port.prepare().unwrap();
/// let rejected = port.try_dispatch(Box::new(|| {}))
///     .expect_err("the default dispatcher rejects work");
/// rejected();
/// port.shutdown().unwrap();
/// ```
pub trait MimeExecutionPort: Debug + Send + Sync + 'static {
    /// Explicitly starts fixed-capacity workers, if required.
    fn prepare(&self) -> Result<(), MimeError>;

    /// Tries to dispatch one owned command or I/O job without blocking.
    /// Returning the job on rejection preserves captured resources.
    ///
    /// # Parameters
    /// - `job`: Owned job to dispatch.
    ///
    /// # Returns
    /// `Ok(())` after acceptance, or the unchanged job after rejection.
    #[inline]
    fn try_dispatch(&self, job: Box<dyn FnOnce() + Send + 'static>) -> Result<(), Box<dyn FnOnce() + Send + 'static>> {
        Err(job)
    }

    /// Tries to dispatch one job and preserves whether rejection means queue
    /// saturation or an unavailable service.
    ///
    /// The default adapter keeps existing implementations source-compatible;
    /// it classifies legacy rejection as `Unavailable` because the old method
    /// does not report a cause.
    #[inline]
    fn try_dispatch_with_status(
        &self,
        job: Box<dyn FnOnce() + Send + 'static>,
    ) -> Result<(), MimeExecutionDispatchFailure> {
        self.try_dispatch(job)
            .map_err(|job| MimeExecutionDispatchFailure::new(MimeExecutionDispatchFailureKind::Unavailable, job))
    }

    /// Stops accepting new work and observes actual worker exit.
    fn shutdown(&self) -> Result<(), MimeError>;
}
