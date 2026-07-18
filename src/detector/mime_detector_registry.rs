// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Runtime Registry and process-wide facade for MIME detector providers.

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
    FileCommandMimeDetectorProvider,
    MimeDetectorProvider,
    MimeDetectorSpec,
    RepositoryMimeDetectorProvider,
};

/// Process-wide MIME detector Registry initialized with built-in providers.
static GLOBAL_MIME_DETECTOR_REGISTRY: LazyLock<MimeDetectorRegistry> =
    LazyLock::new(MimeDetectorRegistry::builtin);

/// Shared runtime Registry for MIME detector provider definitions.
///
/// Clones observe the same underlying provider catalog and default selection.
/// Use [`Self::global`] when App startup registrations must be visible to
/// independently developed downstream libraries.
#[derive(Clone, Debug)]
pub struct MimeDetectorRegistry {
    /// Typed provider Registry owning synchronized runtime state.
    providers: ProviderRegistry<MimeDetectorSpec>,
}

impl MimeDetectorRegistry {
    /// Creates an isolated Registry containing the built-in providers.
    ///
    /// Its stable default selection is the repository-backed provider. This
    /// method does not return the process-wide Registry; use [`Self::global`]
    /// when registrations must cross library boundaries.
    ///
    /// # Returns
    ///
    /// A runtime-mutable Registry containing `repository` and `file`.
    #[must_use]
    pub fn builtin() -> Self {
        let registry = Self::default();
        registry
            .register(RepositoryMimeDetectorProvider)
            .expect("built-in repository MIME provider should register");
        registry
            .register(FileCommandMimeDetectorProvider)
            .expect("built-in file MIME provider should register");
        registry.set_default_selection(
            ProviderSelection::named("repository")
                .expect("built-in repository selection should be valid"),
        );
        registry
    }

    /// Returns the process-wide MIME detector Registry.
    ///
    /// The first access registers built-in providers and selects `repository`
    /// as the stable default. App startup may register additional providers and
    /// replace that default before downstream libraries resolve services.
    ///
    /// # Returns
    ///
    /// The single Registry shared for the lifetime of this process.
    #[inline]
    #[must_use]
    pub fn global() -> &'static Self {
        &GLOBAL_MIME_DETECTOR_REGISTRY
    }

    /// Registers an owned self-described detector provider.
    ///
    /// # Parameters
    ///
    /// * `provider` - Provider definition moved into shared Registry storage.
    ///
    /// # Errors
    ///
    /// Returns [`RegistrationError`] when the provider ID or an alias is
    /// already owned. The Registry remains unchanged on error.
    #[inline(always)]
    pub fn register<P>(&self, provider: P) -> Result<(), RegistrationError>
    where
        P: MimeDetectorProvider,
    {
        self.providers.register(provider)
    }

    /// Registers an already shared self-described detector provider.
    ///
    /// # Parameters
    ///
    /// * `provider` - Type-erased shared provider definition retained by the
    ///   Registry. Concrete owned providers normally use [`Self::register`]
    ///   through the domain-specific [`MimeDetectorProvider`] contract.
    ///
    /// # Errors
    ///
    /// Returns [`RegistrationError`] when the provider ID or an alias is
    /// already owned. The Registry remains unchanged on error.
    #[inline]
    pub fn register_shared(
        &self,
        provider: Arc<dyn ProviderDefinition<MimeDetectorSpec>>,
    ) -> Result<(), RegistrationError> {
        self.providers.register_shared(provider)
    }

    /// Returns the selection used by future default resolutions.
    ///
    /// # Returns
    ///
    /// An owned snapshot independent from any [`crate::MimeConfig`].
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

    /// Resolves one explicit selection into a composing service provider.
    ///
    /// This stage does not create a detector and does not require
    /// [`crate::MimeConfig`].
    ///
    /// # Parameters
    ///
    /// * `selection` - Provider target and creation fallback policy.
    ///
    /// # Returns
    ///
    /// A point-in-time candidate snapshot implementing `ServiceProvider`.
    ///
    /// # Errors
    ///
    /// Returns [`ProviderResolutionError`] when the selection matches no
    /// registered provider.
    #[inline(always)]
    pub fn resolve_selected(
        &self,
        selection: &ProviderSelection,
    ) -> Result<
        ResolvingServiceProvider<MimeDetectorSpec>,
        ProviderResolutionError,
    > {
        self.providers.resolve_selected(selection)
    }

    /// Resolves the Registry's current default selection.
    ///
    /// This stage does not create a detector or inspect service configuration.
    ///
    /// # Returns
    ///
    /// A point-in-time candidate snapshot implementing `ServiceProvider`.
    ///
    /// # Errors
    ///
    /// Returns [`ProviderResolutionError`] when the stored default matches no
    /// registered provider.
    #[inline(always)]
    pub fn resolve(
        &self,
    ) -> Result<
        ResolvingServiceProvider<MimeDetectorSpec>,
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

impl Default for MimeDetectorRegistry {
    /// Creates an empty runtime MIME detector Registry.
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
