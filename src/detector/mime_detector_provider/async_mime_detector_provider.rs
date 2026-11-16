// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Explicit asynchronous backend capability for MIME detector providers.

use std::fmt::Debug;
use std::sync::Arc;

use qubit_spi::ProviderMetadata;

use crate::AsyncMediaStreamClassifierBackend;
use crate::AsyncMimeDetectorBackend;
use crate::MimeCapability;
use crate::MimeConfig;
use crate::MimeError;
use crate::MimeExecutionServices;

/// Explicit async capability offered by a detector provider.
pub trait AsyncMimeDetectorProvider: ProviderMetadata + Debug + Send + Sync {
    /// Creates an async backend bound to this operation service set.
    ///
    /// # Parameters
    /// - `config`: Detector configuration shared with the backend.
    /// - `services`: Prepared execution services for managed requests.
    ///
    /// # Returns
    /// A backend with explicit asynchronous detection support.
    ///
    /// # Errors
    /// Returns the provider's typed MIME error if backend construction fails.
    fn create_async_backend(
        &self,
        config: &MimeConfig,
        services: &MimeExecutionServices,
    ) -> Result<Arc<dyn AsyncMimeDetectorBackend>, MimeError>;

    /// Creates an async detector with an optional async media-stream
    /// classifier attached to its selection core.
    ///
    /// # Parameters
    /// - `config`: Detector configuration shared with the backend.
    /// - `services`: Prepared execution services for managed requests.
    /// - `classifier`: Optional classifier used for media refinement.
    ///
    /// # Returns
    /// A backend configured with the optional asynchronous classifier.
    ///
    /// # Errors
    /// Returns [`MimeError::UnsupportedCapability`] when this provider cannot
    /// attach a requested classifier, or forwards backend construction errors.
    fn create_async_backend_with_classifier(
        &self,
        config: &MimeConfig,
        services: &MimeExecutionServices,
        classifier: Option<Arc<dyn AsyncMediaStreamClassifierBackend>>,
    ) -> Result<Arc<dyn AsyncMimeDetectorBackend>, MimeError> {
        if classifier.is_some() {
            return Err(MimeError::UnsupportedCapability {
                capability: MimeCapability::SameObjectRefinement,
            });
        }
        self.create_async_backend(config, services)
    }
}
