// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Immutable registry for media stream classifier providers.

use std::sync::Arc;

use qubit_spi::error::{
    AttemptFailure,
    ProviderErrorKind,
    ProviderSelectorErrorKind,
    RegistrationError,
    ResolutionError,
};
use qubit_spi::{
    FallbackPolicy,
    ProviderRegistry,
    ProviderResolver,
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
    resolver: ProviderResolver<MediaStreamClassifierSpec>,
}

impl MediaStreamClassifierRegistry {
    /// Creates a registry from providers assembled during application startup.
    #[must_use]
    pub fn new(providers: ProviderRegistry<MediaStreamClassifierSpec>) -> Self {
        let resolver =
            ProviderResolver::new(providers, FallbackPolicy::OnAbsence);
        Self { resolver }
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
        self.resolver
            .registry()
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
        self.resolver
            .create_named(selector, config)
            .map(|created| created.into_service())
            .map_err(classifier_resolution_error)
    }

    /// Creates a classifier using the configured selector or automatic order.
    pub fn create_default(
        &self,
        config: &MimeConfig,
    ) -> MimeResult<Arc<dyn MediaStreamClassifier>> {
        let configured = config.media_stream_classifier_default().trim();
        let created = if configured.is_empty()
            || configured.eq_ignore_ascii_case("auto")
        {
            self.resolver.create_auto(config)
        } else {
            self.resolver.create_named(configured, config)
        };
        created
            .map(|created| created.into_service())
            .map_err(classifier_resolution_error)
    }
}

pub(super) fn classifier_registration_error(
    error: RegistrationError,
) -> MimeError {
    MimeError::DuplicateClassifierName {
        name: error.selector().to_owned(),
    }
}

fn classifier_resolution_error(error: ResolutionError) -> MimeError {
    let message = error.to_string();
    match error {
        ResolutionError::InvalidSelector { input, source, .. } => {
            if source.kind() == ProviderSelectorErrorKind::Empty {
                MimeError::EmptyClassifierName
            } else {
                MimeError::InvalidClassifierName {
                    name: input.into(),
                    reason: message,
                }
            }
        }
        ResolutionError::UnknownProvider { selector } => {
            MimeError::UnknownClassifier {
                name: selector.as_str().to_owned(),
            }
        }
        ResolutionError::EmptySelection | ResolutionError::EmptyRegistry => {
            MimeError::NoAvailableClassifier { reason: message }
        }
        ResolutionError::NoProviderSucceeded { attempts } => {
            match attempts.as_ref() {
                [
                    AttemptFailure::ProviderError {
                        provider_id, error, ..
                    },
                ] => match error.kind() {
                    ProviderErrorKind::Unsupported
                    | ProviderErrorKind::Unavailable => {
                        MimeError::ClassifierUnavailable {
                            name: provider_id.as_str().to_owned(),
                            reason: error.reason().to_owned(),
                        }
                    }
                    _ => MimeError::ClassifierBackend {
                        backend: provider_id.as_str().to_owned(),
                        reason: error.reason().to_owned(),
                    },
                },
                _ => MimeError::NoAvailableClassifier { reason: message },
            }
        }
    }
}
