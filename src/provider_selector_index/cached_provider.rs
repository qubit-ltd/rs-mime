// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Cached synchronous provider metadata.

use std::sync::Arc;

use qubit_spi::ProviderDefinition;
use qubit_spi::ProviderDescriptor;
use qubit_spi::ProviderMetadata;
use qubit_spi::ServiceProvider;
use qubit_spi::SyncServiceSpec;
use qubit_spi::error::ProviderFailure;

/// Sync provider adapter that returns captured metadata without running
/// provider callbacks while the shared registration boundary is held.
///
/// # Type Parameters
/// - `S`: Service specification produced by the wrapped provider.
pub(crate) struct CachedProvider<S: SyncServiceSpec> {
    /// Provider factory used after registration locks have been released.
    provider: Arc<dyn ProviderDefinition<S>>,
    /// Descriptor captured before the provider was inserted into its registry.
    descriptor: ProviderDescriptor,
}

impl<S: SyncServiceSpec> CachedProvider<S> {
    /// Creates an adapter retaining the provider and its registration
    /// descriptor.
    ///
    /// # Parameters
    /// - `provider`: Registered provider whose factory remains available.
    /// - `descriptor`: Metadata captured before the registration lock is held.
    ///
    /// # Returns
    /// An adapter that serves the cached descriptor and delegates construction.
    pub(crate) fn new(provider: Arc<dyn ProviderDefinition<S>>, descriptor: ProviderDescriptor) -> Self {
        Self { provider, descriptor }
    }
}

impl<S: SyncServiceSpec> ProviderMetadata for CachedProvider<S> {
    /// Returns the descriptor captured when this adapter was registered.
    fn descriptor(&self) -> ProviderDescriptor {
        self.descriptor.clone()
    }
}

impl<S: SyncServiceSpec> ServiceProvider<S> for CachedProvider<S> {
    /// Creates an instance using the wrapped provider's factory.
    ///
    /// # Parameters
    /// - `config`: Service configuration passed to the provider factory.
    ///
    /// # Returns
    /// A configured service or the provider's original construction error.
    fn create_configured(&self, config: &S::Config) -> Result<S::Output, ProviderFailure<S::Error>> {
        self.provider.create_configured(config)
    }
}
