// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Immutable registry for pluggable MIME detector providers.

use std::sync::Arc;

use qubit_spi::{
    FallbackPolicy,
    ProviderErrorKind,
    ProviderRegistry,
    ProviderResolver,
    ProviderSelection,
    RegistrationError,
    RegistrationErrorKind,
    ResolutionError,
    ResolutionErrorKind,
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
        let resolver = ProviderResolver::new(providers, FallbackPolicy::OnAbsence);
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
        let selection = ProviderSelection::named(selector)
            .map_err(detector_registration_error)?;
        self.resolver
            .create(&selection, config)
            .map(|created| created.into_service())
            .map_err(detector_resolution_error)
    }

    /// Creates a detector using configured automatic or fallback selection.
    pub fn create_default(
        &self,
        config: &MimeConfig,
    ) -> MimeResult<Arc<dyn MimeDetector>> {
        let selection = detector_selection(config)?;
        self.resolver
            .create(&selection, config)
            .map(|created| created.into_service())
            .map_err(detector_resolution_error)
    }
}

fn detector_selection(config: &MimeConfig) -> MimeResult<ProviderSelection> {
    let primary = config.mime_detector_default().trim();
    if primary.is_empty() || primary.eq_ignore_ascii_case("auto") {
        return Ok(ProviderSelection::Auto);
    }
    ProviderSelection::chain(
        std::iter::once(primary)
            .chain(config.mime_detector_fallbacks().iter().map(String::as_str)),
    )
    .map_err(detector_registration_error)
}

pub(crate) fn detector_registration_error(
    error: RegistrationError,
) -> MimeError {
    match error.kind() {
        RegistrationErrorKind::EmptyIdentifier => MimeError::EmptyDetectorName,
        RegistrationErrorKind::InvalidIdentifier => {
            MimeError::InvalidDetectorName {
                name: error.identifier().unwrap_or_default().to_owned(),
                reason: error.to_string(),
            }
        }
        RegistrationErrorKind::DuplicateSelector => {
            MimeError::DuplicateDetectorName {
                name: error.identifier().unwrap_or_default().to_owned(),
            }
        }
        _ => MimeError::DetectorBackend {
            backend: "registry".into(),
            reason: error.to_string(),
        },
    }
}

pub(crate) fn detector_resolution_error(error: ResolutionError) -> MimeError {
    if matches!(
        error.kind(),
        ResolutionErrorKind::InvalidSelector | ResolutionErrorKind::UnknownProvider
    ) {
        return MimeError::UnknownDetector {
            name: error
                .selector_input()
                .unwrap_or("<invalid>")
                .to_owned(),
        };
    }
    let attempts = error.attempts();
    if let [attempt] = attempts {
        let name = attempt
            .provider_id()
            .map_or("<unknown>", |id| id.as_str())
            .to_owned();
        return match attempt.provider_error_kind() {
            Some(
                ProviderErrorKind::Unsupported | ProviderErrorKind::Unavailable,
            ) => MimeError::DetectorUnavailable {
                name,
                reason: attempt.reason().to_owned(),
            },
            _ => MimeError::DetectorBackend {
                backend: name,
                reason: attempt.reason().to_owned(),
            },
        };
    }
    MimeError::NoAvailableDetector {
        reason: attempts
            .iter()
            .map(|attempt| attempt.reason())
            .collect::<Vec<_>>()
            .join("; "),
    }
}
