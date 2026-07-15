// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Startup builder for MIME detector providers.

use std::sync::Arc;

use qubit_spi::{
    ProviderDescriptor,
    ProviderRegistryBuilder,
    ServiceProvider,
};

use crate::{
    MimeResult,
    detector::mime_detector_registry::detector_registration_error,
};

use super::{
    MimeDetectorRegistry,
    MimeDetectorSpec,
};

/// Startup-only builder for an immutable MIME detector registry.
#[derive(Default)]
pub struct MimeDetectorRegistryBuilder {
    providers: ProviderRegistryBuilder<MimeDetectorSpec>,
}

impl MimeDetectorRegistryBuilder {
    /// Creates an empty MIME detector provider builder.
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
        P: ServiceProvider<MimeDetectorSpec>,
    {
        self.providers
            .register(descriptor, provider)
            .map_err(detector_registration_error)
    }

    /// Registers an already shared provider factory.
    pub fn register_shared(
        &mut self,
        descriptor: ProviderDescriptor,
        provider: Arc<dyn ServiceProvider<MimeDetectorSpec>>,
    ) -> MimeResult<()> {
        self.providers
            .register_shared(descriptor, provider)
            .map_err(detector_registration_error)
    }

    /// Builds the runtime immutable registry.
    #[must_use]
    pub fn build(self) -> MimeDetectorRegistry {
        MimeDetectorRegistry::new(self.providers.build())
    }
}
