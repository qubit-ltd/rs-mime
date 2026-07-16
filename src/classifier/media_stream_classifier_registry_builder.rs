// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Startup builder for media stream classifier providers.

use std::sync::Arc;

use qubit_spi::error::RegistrationError;
use qubit_spi::{
    ProviderDescriptor,
    ProviderRegistryBuilder,
    ServiceProvider,
};

use crate::{
    MimeError,
    MimeResult,
};

use super::{
    MediaStreamClassifierRegistry,
    MediaStreamClassifierSpec,
};

/// Startup-only builder for an immutable media stream classifier registry.
#[derive(Default)]
pub struct MediaStreamClassifierRegistryBuilder {
    providers: ProviderRegistryBuilder<MediaStreamClassifierSpec>,
}

/// Maps an SPI registration conflict into the classifier error model.
fn classifier_registration_error(error: RegistrationError) -> MimeError {
    let reason = error.to_string();
    match error {
        RegistrationError::DuplicateSelector { selector, .. } => {
            MimeError::DuplicateClassifierName {
                name: selector.into(),
            }
        }
        _ => MimeError::NoAvailableClassifier { reason },
    }
}

impl MediaStreamClassifierRegistryBuilder {
    /// Creates an empty classifier provider builder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers provider metadata and its factory.
    pub fn register<P>(
        &mut self,
        descriptor: ProviderDescriptor,
        provider: P,
    ) -> MimeResult<()>
    where
        P: ServiceProvider<MediaStreamClassifierSpec>,
    {
        self.providers
            .register(descriptor, provider)
            .map_err(classifier_registration_error)
    }

    /// Registers an already shared provider factory.
    pub fn register_shared(
        &mut self,
        descriptor: ProviderDescriptor,
        provider: Arc<dyn ServiceProvider<MediaStreamClassifierSpec>>,
    ) -> MimeResult<()> {
        self.providers
            .register_shared(descriptor, provider)
            .map_err(classifier_registration_error)
    }

    /// Builds the runtime immutable registry.
    #[must_use]
    pub fn build(self) -> MediaStreamClassifierRegistry {
        MediaStreamClassifierRegistry::new(self.providers.build())
    }
}
