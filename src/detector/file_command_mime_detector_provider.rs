// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Provider for the built-in system `file` command MIME detector.

use std::sync::Arc;

use qubit_spi::ProviderDescriptor;
use qubit_spi::ProviderMetadata;
use qubit_spi::ServiceProvider;
use qubit_spi::error::ProviderFailure;
use qubit_spi::provider_descriptor;

use super::MimeDetectorSpec;
use crate::AsyncMimeDetectorBackend;
use crate::FileCommandMimeDetector;
use crate::MimeConfig;
use crate::MimeDetector;
use crate::MimeError;
use crate::MimeExecutionServices;

/// Provider for the built-in system `file` command detector.
///
/// # Examples
///
/// ```
/// use qubit_mime::FileCommandMimeDetectorProvider;
/// use qubit_spi::ProviderMetadata;
///
/// let provider = FileCommandMimeDetectorProvider;
/// let descriptor = provider.descriptor();
/// assert_eq!(descriptor.id().as_str(), "file");
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct FileCommandMimeDetectorProvider;

impl FileCommandMimeDetectorProvider {
    /// Creates the explicit async backend using caller-prepared MIME services.
    ///
    /// # Parameters
    /// - `config`: MIME detector configuration.
    /// - `services`: Prepared services for managed asynchronous execution.
    ///
    /// # Returns
    /// A backend configured with the supplied MIME services.
    pub fn create_async_backend(
        &self,
        config: &MimeConfig,
        services: MimeExecutionServices,
    ) -> Arc<dyn AsyncMimeDetectorBackend> {
        Arc::new(
            FileCommandMimeDetector::from_mime_config(config.clone())
                .with_execution_services(services),
        )
    }
}

impl ServiceProvider<MimeDetectorSpec> for FileCommandMimeDetectorProvider {
    /// Creates a detector backed by the system `file` command.
    ///
    /// Returns an unavailable provider failure when the command cannot be
    /// executed in the current environment.
    ///
    /// # Parameters
    /// - `config`: Configuration used to create the detector.
    ///
    /// # Returns
    /// A shared MIME detector configured to use the system `file` command.
    ///
    /// # Errors
    /// Returns an unavailable provider failure if the system `file` command
    /// cannot be executed in the current environment.
    fn create_configured(
        &self,
        config: &MimeConfig,
    ) -> Result<Arc<dyn MimeDetector>, ProviderFailure<MimeError>> {
        if !FileCommandMimeDetector::is_available_with_config(config) {
            return Err(ProviderFailure::unavailable(
                MimeError::DetectorUnavailable {
                    name: "file".to_owned(),
                    reason: "`file` command is not available".to_owned(),
                },
            ));
        }
        Ok(Arc::new(FileCommandMimeDetector::from_mime_config(
            config.clone(),
        )))
    }
}

impl ProviderMetadata for FileCommandMimeDetectorProvider {
    /// Returns the stable identity and automatic-selection priority.
    ///
    /// # Returns
    ///
    /// The `file` provider descriptor and its accepted aliases.
    fn descriptor(&self) -> ProviderDescriptor {
        provider_descriptor!(
            "file",
            aliases: ["file-command", "file-command-mime-detector"],
            priority: 10,
        )
    }
}

impl super::AsyncMimeDetectorProvider for FileCommandMimeDetectorProvider {
    /// Creates an async backend using the supplied configuration and services.
    ///
    /// # Parameters
    /// - `config`: MIME detector configuration.
    /// - `services`: Services used for managed asynchronous execution.
    ///
    /// # Returns
    /// The configured backend.
    ///
    /// # Errors
    /// This provider currently has no backend construction errors.
    fn create_async_backend(
        &self,
        config: &MimeConfig,
        services: &MimeExecutionServices,
    ) -> Result<Arc<dyn AsyncMimeDetectorBackend>, MimeError> {
        Ok(Arc::new(
            FileCommandMimeDetector::from_mime_config(config.clone())
                .with_execution_services(services.clone()),
        ))
    }

    /// Creates an async backend and validates optional classifier support.
    ///
    /// # Parameters
    /// - `config`: MIME detector configuration.
    /// - `services`: Services used for managed asynchronous execution.
    /// - `classifier`: Optional same-object classifier backend.
    ///
    /// # Returns
    /// The configured backend when no unsupported classifier is requested.
    ///
    /// # Errors
    /// Returns an unsupported-capability error when a classifier is supplied.
    fn create_async_backend_with_classifier(
        &self,
        _config: &MimeConfig,
        _services: &MimeExecutionServices,
        classifier: Option<Arc<dyn crate::AsyncMediaStreamClassifierBackend>>,
    ) -> Result<Arc<dyn AsyncMimeDetectorBackend>, MimeError> {
        if classifier.is_some() {
            return Err(MimeError::UnsupportedCapability {
                capability: crate::MimeCapability::SameObjectRefinement,
            });
        }
        Ok(self.create_async_backend(_config, _services.clone()))
    }
}
