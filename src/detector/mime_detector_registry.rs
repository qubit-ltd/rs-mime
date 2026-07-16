// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Immutable registry for pluggable MIME detector providers.

use std::sync::Arc;

use qubit_spi::error::{
    AttemptFailure,
    ProviderErrorKind,
    RegistrationError,
    ResolutionError,
};
use qubit_spi::{
    FallbackPolicy,
    ProviderRegistry,
    ProviderResolver,
};

use crate::{
    MimeConfig,
    MimeDetector,
    MimeError,
    MimeResult,
};

use super::{
    FileCommandMimeDetectorProvider,
    MimeDetectorRegistryBuilder,
    MimeDetectorSpec,
    RepositoryMimeDetectorProvider,
    file_command_mime_detector_descriptor,
    repository_mime_detector_descriptor,
};

/// Immutable registry of MIME detector providers.
pub struct MimeDetectorRegistry {
    resolver: ProviderResolver<MimeDetectorSpec>,
}

impl MimeDetectorRegistry {
    /// Creates a registry from providers assembled during application startup.
    #[must_use]
    pub fn new(providers: ProviderRegistry<MimeDetectorSpec>) -> Self {
        let resolver =
            ProviderResolver::new(providers, FallbackPolicy::OnAbsence);
        Self { resolver }
    }

    /// Creates a startup-only builder for MIME detector providers.
    #[must_use]
    pub fn builder() -> MimeDetectorRegistryBuilder {
        MimeDetectorRegistryBuilder::new()
    }

    /// Creates a registry containing the repository and `file` providers.
    #[must_use]
    pub fn builtin() -> Self {
        let mut builder = Self::builder();
        builder
            .register(
                repository_mime_detector_descriptor(),
                RepositoryMimeDetectorProvider,
            )
            .expect("built-in repository MIME provider should register");
        builder
            .register(
                file_command_mime_detector_descriptor(),
                FileCommandMimeDetectorProvider,
            )
            .expect("built-in file MIME provider should register");
        builder.build()
    }

    /// Lists canonical provider IDs in registration order.
    #[must_use]
    pub fn provider_ids(&self) -> Vec<&str> {
        self.resolver
            .registry()
            .provider_ids()
            .map(|id| id.as_str())
            .collect()
    }

    /// Creates a detector through one explicit provider ID or alias.
    pub fn create(
        &self,
        selector: &str,
        config: &MimeConfig,
    ) -> MimeResult<Arc<dyn MimeDetector>> {
        self.resolver
            .create_named(selector, config)
            .map(|created| created.into_service())
            .map_err(detector_resolution_error)
    }

    /// Creates a detector using configured automatic or fallback selection.
    pub fn create_default(
        &self,
        config: &MimeConfig,
    ) -> MimeResult<Arc<dyn MimeDetector>> {
        self.resolver
            .create(config.mime_detector_selection(), config)
            .map(|created| created.into_service())
            .map_err(detector_resolution_error)
    }
}

pub(crate) fn detector_registration_error(
    error: RegistrationError,
) -> MimeError {
    MimeError::DuplicateDetectorName {
        name: error.selector().to_owned(),
    }
}

pub(crate) fn detector_resolution_error(error: ResolutionError) -> MimeError {
    let message = error.to_string();
    match &error {
        ResolutionError::InvalidSelector { input, source, .. } => {
            if source.is_empty() {
                MimeError::EmptyDetectorName
            } else {
                MimeError::InvalidDetectorName {
                    name: input.to_string(),
                    reason: message,
                }
            }
        }
        ResolutionError::UnknownProvider { selector } => {
            MimeError::UnknownDetector {
                name: selector.as_str().to_owned(),
            }
        }
        ResolutionError::EmptySelection | ResolutionError::EmptyRegistry => {
            MimeError::NoAvailableDetector { reason: message }
        }
        ResolutionError::NoProviderSucceeded { .. } => error
            .decisive_attempt()
            .map(detector_attempt_error)
            .unwrap_or(MimeError::NoAvailableDetector { reason: message }),
        _ => MimeError::NoAvailableDetector { reason: message },
    }
}

/// Maps one failed SPI attempt into a precise detector-domain error.
///
/// # Arguments
///
/// * `attempt` - Failed lookup or provider creation attempt.
///
/// # Returns
///
/// A precise domain error when the attempt exposes its required context.
fn detector_attempt_error(attempt: &AttemptFailure) -> MimeError {
    match attempt {
        AttemptFailure::UnknownProvider { requested_selector } => {
            MimeError::UnknownDetector {
                name: requested_selector.as_str().to_owned(),
            }
        }
        AttemptFailure::ProviderError {
            provider_id, error, ..
        } => match error.kind() {
            ProviderErrorKind::Unsupported | ProviderErrorKind::Unavailable => {
                MimeError::DetectorUnavailable {
                    name: provider_id.as_str().to_owned(),
                    reason: error.reason().to_owned(),
                }
            }
            _ => MimeError::DetectorBackend {
                backend: provider_id.as_str().to_owned(),
                reason: error.reason().to_owned(),
            },
        },
        _ => MimeError::NoAvailableDetector {
            reason: attempt.to_string(),
        },
    }
}
