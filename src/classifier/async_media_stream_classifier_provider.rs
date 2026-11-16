// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Explicit async capability offered by a media classifier provider.

use std::fmt::Debug;
use std::sync::Arc;

use qubit_spi::ProviderMetadata;

use crate::AsyncMediaStreamClassifierBackend;
use crate::MimeConfig;
use crate::MimeError;
use crate::MimeExecutionServices;

/// Provider contract for constructing operation-bound asynchronous classifiers.
///
/// Implement this trait when a provider can create an asynchronous backend
/// using the operation execution services supplied by the caller.
///
/// # Examples
///
/// ```
/// use std::sync::Arc;
///
/// use qubit_mime::AsyncMediaStreamClassifierBackend;
/// use qubit_mime::AsyncMediaStreamClassifierProvider;
/// use qubit_mime::MimeConfig;
/// use qubit_mime::MimeError;
/// use qubit_mime::MimeExecutionServices;
///
/// fn create_backend(
///     provider: &dyn AsyncMediaStreamClassifierProvider,
///     config: &MimeConfig,
///     services: &MimeExecutionServices,
/// ) -> Result<Arc<dyn AsyncMediaStreamClassifierBackend>, MimeError> {
///     provider.create_async_backend(config, services)
/// }
/// ```
pub trait AsyncMediaStreamClassifierProvider: ProviderMetadata + Debug + Send + Sync {
    /// Creates an async classifier bound to operation execution services.
    ///
    /// # Parameters
    /// - `config`: MIME limits and settings used to configure the backend.
    /// - `services`: Operation execution and resource services for async work.
    ///
    /// # Returns
    /// A backend that performs classification within the supplied services.
    ///
    /// # Errors
    /// Returns a provider or configuration error when the backend cannot be
    /// constructed for these services.
    fn create_async_backend(
        &self,
        config: &MimeConfig,
        services: &MimeExecutionServices,
    ) -> Result<Arc<dyn AsyncMediaStreamClassifierBackend>, MimeError>;
}
