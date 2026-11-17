// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! MIME-owned command and I/O execution boundary.

use std::fmt::Debug;

use crate::MimeError;

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

    /// Stops accepting new work and observes actual worker exit.
    fn shutdown(&self) -> Result<(), MimeError>;
}
