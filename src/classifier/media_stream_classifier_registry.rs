// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Immutable registry for media stream classifier providers.

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
    MediaStreamClassifier,
    MimeConfig,
    MimeError,
    MimeResult,
};

use super::{
    FfprobeCommandMediaStreamClassifierProvider,
    MediaStreamClassifierRegistryBuilder,
    MediaStreamClassifierSpec,
    ffprobe_command_media_stream_classifier_descriptor,
};

/// Immutable registry of media stream classifier providers.
pub struct MediaStreamClassifierRegistry {
    providers: ProviderRegistry<MediaStreamClassifierSpec>,
    resolver: ProviderResolver<MediaStreamClassifierSpec>,
}

impl MediaStreamClassifierRegistry {
    /// Creates a registry from providers assembled during application startup.
    #[must_use]
    pub fn new(providers: ProviderRegistry<MediaStreamClassifierSpec>) -> Self {
        let resolver =
            ProviderResolver::new(providers.clone(), FallbackPolicy::OnAbsence);
        Self {
            providers,
            resolver,
        }
    }

    /// Creates a startup-only builder for classifier providers.
    #[must_use]
    pub fn builder() -> MediaStreamClassifierRegistryBuilder {
        MediaStreamClassifierRegistryBuilder::new()
    }

    /// Creates a registry containing the FFprobe provider.
    #[must_use]
    pub fn builtin() -> Self {
        let mut builder = Self::builder();
        builder
            .register(
                ffprobe_command_media_stream_classifier_descriptor(),
                FfprobeCommandMediaStreamClassifierProvider,
            )
            .expect("built-in FFprobe classifier provider should register");
        builder.build()
    }

    /// Lists canonical provider IDs in registration order.
    #[must_use]
    pub fn provider_ids(&self) -> Vec<&str> {
        self.providers
            .provider_ids()
            .map(|id| id.as_str())
            .collect()
    }

    /// Creates a classifier through one explicit provider ID or alias.
    pub fn create(
        &self,
        selector: &str,
        config: &MimeConfig,
    ) -> MimeResult<Arc<dyn MediaStreamClassifier>> {
        let selection = ProviderSelection::named(selector)
            .map_err(classifier_registration_error)?;
        self.resolver
            .create(&selection, config)
            .map(|created| created.into_service())
            .map_err(classifier_resolution_error)
    }

    /// Creates a classifier using the configured selector or automatic order.
    pub fn create_default(
        &self,
        config: &MimeConfig,
    ) -> MimeResult<Arc<dyn MediaStreamClassifier>> {
        let configured = config.media_stream_classifier_default().trim();
        let selection = if configured.is_empty()
            || configured.eq_ignore_ascii_case("auto")
        {
            ProviderSelection::Auto
        } else {
            ProviderSelection::named(configured)
                .map_err(classifier_registration_error)?
        };
        self.resolver
            .create(&selection, config)
            .map(|created| created.into_service())
            .map_err(classifier_resolution_error)
    }
}

pub(super) fn classifier_registration_error(
    error: RegistrationError,
) -> MimeError {
    match error.kind() {
        RegistrationErrorKind::EmptyIdentifier => {
            MimeError::EmptyClassifierName
        }
        RegistrationErrorKind::InvalidIdentifier => {
            MimeError::InvalidClassifierName {
                name: error.identifier().unwrap_or_default().to_owned(),
                reason: error.to_string(),
            }
        }
        RegistrationErrorKind::DuplicateSelector => {
            MimeError::DuplicateClassifierName {
                name: error.identifier().unwrap_or_default().to_owned(),
            }
        }
        _ => MimeError::ClassifierBackend {
            backend: "registry".into(),
            reason: error.to_string(),
        },
    }
}

fn classifier_resolution_error(error: ResolutionError) -> MimeError {
    if error.kind() == ResolutionErrorKind::UnknownProvider {
        return MimeError::UnknownClassifier {
            name: error
                .requested_selector()
                .map_or("<invalid>", |selector| selector.as_str())
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
            ) => MimeError::ClassifierUnavailable {
                name,
                reason: attempt.reason().to_owned(),
            },
            _ => MimeError::ClassifierBackend {
                backend: name,
                reason: attempt.reason().to_owned(),
            },
        };
    }
    MimeError::NoAvailableClassifier {
        reason: attempts
            .iter()
            .map(|attempt| attempt.reason())
            .collect::<Vec<_>>()
            .join("; "),
    }
}
