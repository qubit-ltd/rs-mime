// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Optional assembly builder for media stream classifier providers.

use std::sync::Arc;

use qubit_spi::error::RegistrationError;
use qubit_spi::{
    ProviderDefinition,
    ProviderRegistryBuilder,
};

use super::{
    MediaStreamClassifierProvider,
    MediaStreamClassifierRegistry,
    MediaStreamClassifierSpec,
};

/// Optional builder for an initially assembled runtime classifier Registry.
#[derive(Default)]
pub struct MediaStreamClassifierRegistryBuilder {
    /// Typed provider builder receiving self-described definitions.
    providers: ProviderRegistryBuilder<MediaStreamClassifierSpec>,
}

impl MediaStreamClassifierRegistryBuilder {
    /// Creates an empty classifier provider builder.
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers one owned self-described classifier provider.
    ///
    /// # Parameters
    ///
    /// * `provider` - Provider definition moved into Registry storage.
    ///
    /// # Errors
    ///
    /// Returns [`RegistrationError`] when its ID or an alias is already owned.
    #[inline(always)]
    pub fn register<P>(&mut self, provider: P) -> Result<(), RegistrationError>
    where
        P: MediaStreamClassifierProvider,
    {
        self.providers.register(provider)
    }

    /// Registers one already shared self-described classifier provider.
    ///
    /// # Parameters
    ///
    /// * `provider` - Type-erased shared provider definition retained by the
    ///   Registry. Concrete owned providers normally use [`Self::register`]
    ///   through the domain-specific [`MediaStreamClassifierProvider`]
    ///   contract.
    ///
    /// # Errors
    ///
    /// Returns [`RegistrationError`] when its ID or an alias is already owned.
    #[inline]
    pub fn register_shared(
        &mut self,
        provider: Arc<dyn ProviderDefinition<MediaStreamClassifierSpec>>,
    ) -> Result<(), RegistrationError> {
        self.providers.register_shared(provider)
    }

    /// Builds the runtime-mutable classifier Registry.
    ///
    /// # Returns
    ///
    /// A Registry that remains open to future registrations.
    #[must_use]
    pub fn build(self) -> MediaStreamClassifierRegistry {
        MediaStreamClassifierRegistry::new(self.providers.build())
    }
}
