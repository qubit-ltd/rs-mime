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
    AttemptFailureKind,
    FallbackPolicy,
    ProviderErrorKind,
    ProviderRegistry,
    ProviderResolver,
    ProviderSelectorErrorKind,
    RegistrationError,
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
        let primary = config.mime_detector_default().trim();
        let created = if primary.is_empty() || primary.eq_ignore_ascii_case("auto") {
            self.resolver.create_auto(config)
        } else {
            self.resolver.create_chain(
                std::iter::once(primary)
                    .chain(config.mime_detector_fallbacks().iter().map(String::as_str)),
                config,
            )
        };
        created
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
    if error.kind() == ResolutionErrorKind::InvalidSelector {
        if error
            .selector_error()
            .is_some_and(|source| source.kind() == ProviderSelectorErrorKind::Empty)
        {
            return MimeError::EmptyDetectorName;
        }
        return MimeError::InvalidDetectorName {
            name: error
                .selector_input()
                .unwrap_or("<invalid>")
                .to_owned(),
            reason: error.to_string(),
        };
    }
    if error.kind() == ResolutionErrorKind::UnknownProvider {
        return MimeError::UnknownDetector {
            name: error.selector_input().unwrap_or("<unknown>").to_owned(),
        };
    }
    let attempts = error.attempts();
    if let [attempt] = attempts {
        if attempt.kind() == AttemptFailureKind::UnknownProvider {
            return MimeError::UnknownDetector {
                name: attempt
                    .requested_selector()
                    .map_or("<unknown>", |selector| selector.as_str())
                    .to_owned(),
            };
        }
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
        reason: error.to_string(),
    }
}
