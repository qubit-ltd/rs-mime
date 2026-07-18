// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Runtime Registry and global facade for media stream classifier providers.

use std::sync::{
    Arc,
    LazyLock,
};

use qubit_spi::error::{
    ProviderResolutionError,
    RegistrationError,
};
use qubit_spi::{
    ProviderDefinition,
    ProviderId,
    ProviderRegistry,
    ProviderSelection,
    ResolvingServiceProvider,
};

use super::{
    FfprobeCommandMediaStreamClassifierProvider,
    MediaStreamClassifierProvider,
    MediaStreamClassifierSpec,
};

/// Process-wide classifier Registry initialized with the built-in provider.
static GLOBAL_MEDIA_STREAM_CLASSIFIER_REGISTRY: LazyLock<
    MediaStreamClassifierRegistry,
> = LazyLock::new(MediaStreamClassifierRegistry::builtin);

/// Shared runtime Registry for media stream classifier definitions.
///
/// Clones observe the same synchronized provider catalog and default
/// selection. Use [`Self::global`] for App startup registrations intended for
/// independently developed downstream libraries.
#[derive(Clone, Debug)]
pub struct MediaStreamClassifierRegistry {
    /// Typed provider Registry owning synchronized runtime state.
    providers: ProviderRegistry<MediaStreamClassifierSpec>,
}

impl MediaStreamClassifierRegistry {
    /// Creates an isolated Registry containing the FFprobe provider.
    ///
    /// Its stable default selection is `ffprobe`. This does not return the
    /// process-wide Registry; use [`Self::global`] for cross-library state.
    ///
    /// # Returns
    ///
    /// A runtime-mutable Registry containing the FFprobe provider.
    #[must_use]
    pub fn builtin() -> Self {
        let registry = Self::default();
        registry
            .register(FfprobeCommandMediaStreamClassifierProvider)
            .expect("built-in FFprobe classifier provider should register");
        registry.set_default_selection(
            ProviderSelection::named("ffprobe")
                .expect("built-in FFprobe selection should be valid"),
        );
        registry
    }

    /// Returns the process-wide media stream classifier Registry.
    ///
    /// # Returns
    ///
    /// The single Registry shared for the lifetime of this process.
    #[inline]
    #[must_use]
    pub fn global() -> &'static Self {
        &GLOBAL_MEDIA_STREAM_CLASSIFIER_REGISTRY
    }

    /// Registers an owned self-described classifier provider.
    ///
    /// # Parameters
    ///
    /// * `provider` - Provider definition moved into Registry storage.
    ///
    /// # Errors
    ///
    /// Returns [`RegistrationError`] when its ID or an alias is already owned.
    #[inline(always)]
    pub fn register<P>(&self, provider: P) -> Result<(), RegistrationError>
    where
        P: MediaStreamClassifierProvider,
    {
        self.providers.register(provider)
    }

    /// Registers an already shared self-described classifier provider.
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
        &self,
        provider: Arc<dyn ProviderDefinition<MediaStreamClassifierSpec>>,
    ) -> Result<(), RegistrationError> {
        self.providers.register_shared(provider)
    }

    /// Returns the selection used by future default resolutions.
    ///
    /// # Returns
    ///
    /// An owned snapshot independent from [`crate::MimeConfig`].
    #[inline(always)]
    #[must_use]
    pub fn default_selection(&self) -> ProviderSelection {
        self.providers.default_selection()
    }

    /// Replaces the selection used by future default resolutions.
    ///
    /// # Parameters
    ///
    /// * `selection` - Validated selection and creation fallback policy.
    #[inline(always)]
    pub fn set_default_selection(&self, selection: ProviderSelection) {
        self.providers.set_default_selection(selection);
    }

    /// Resolves an explicit selection into a composing service provider.
    ///
    /// This stage does not create a classifier and does not inspect
    /// [`crate::MimeConfig`].
    ///
    /// # Parameters
    ///
    /// * `selection` - Provider target and creation fallback policy.
    ///
    /// # Returns
    ///
    /// A point-in-time classifier candidate snapshot.
    ///
    /// # Errors
    ///
    /// Returns [`ProviderResolutionError`] when no candidate matches.
    #[inline(always)]
    pub fn resolve_selected(
        &self,
        selection: &ProviderSelection,
    ) -> Result<
        ResolvingServiceProvider<MediaStreamClassifierSpec>,
        ProviderResolutionError,
    > {
        self.providers.resolve_selected(selection)
    }

    /// Resolves the Registry's current default selection.
    ///
    /// This stage does not create a classifier or inspect service config.
    ///
    /// # Returns
    ///
    /// A point-in-time classifier candidate snapshot.
    ///
    /// # Errors
    ///
    /// Returns [`ProviderResolutionError`] when the stored default matches no
    /// registered provider.
    #[inline(always)]
    pub fn resolve(
        &self,
    ) -> Result<
        ResolvingServiceProvider<MediaStreamClassifierSpec>,
        ProviderResolutionError,
    > {
        self.providers.resolve()
    }

    /// Lists canonical provider IDs in registration order.
    ///
    /// # Returns
    ///
    /// An owned provider-ID snapshot unaffected by later registrations.
    #[inline]
    #[must_use]
    pub fn provider_ids(&self) -> Vec<ProviderId> {
        self.providers.provider_ids()
    }
}

impl Default for MediaStreamClassifierRegistry {
    /// Creates an empty runtime media stream classifier Registry.
    ///
    /// # Returns
    ///
    /// A Registry with automatic selection and no providers.
    #[inline]
    fn default() -> Self {
        Self {
            providers: ProviderRegistry::default(),
        }
    }
}
