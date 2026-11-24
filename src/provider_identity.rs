// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Shared selector identity checks for synchronous and asynchronous providers.

use std::collections::HashMap;
use std::sync::Arc;

use qubit_spi::ProviderDefinition;
use qubit_spi::ProviderDescriptor;
use qubit_spi::ProviderMetadata;
use qubit_spi::ServiceProvider;
use qubit_spi::SyncServiceSpec;
use qubit_spi::error::ProviderFailure;

/// Selector ownership shared by both provider capabilities.
#[derive(Clone, Debug, Default)]
pub(crate) struct SelectorIdentityIndex {
    selectors: HashMap<String, String>,
}

impl SelectorIdentityIndex {
    /// Rejects selectors already associated with another canonical ID.
    pub(crate) fn validate(&self, descriptor: &ProviderDescriptor) -> Result<(), (String, String)> {
        let id = descriptor.id().as_str();
        for selector in
            std::iter::once(id).chain(descriptor.aliases().iter().map(|alias| alias.as_str()))
        {
            if let Some(owner) = self.selectors.get(selector) {
                if owner != id {
                    return Err((selector.to_owned(), owner.clone()));
                }
            }
        }
        Ok(())
    }

    /// Adds selector ownership after registration succeeds.
    pub(crate) fn insert(&mut self, descriptor: &ProviderDescriptor) {
        let id = descriptor.id().as_str().to_owned();
        for selector in std::iter::once(id.as_str())
            .chain(descriptor.aliases().iter().map(|alias| alias.as_str()))
        {
            self.selectors.insert(selector.to_owned(), id.clone());
        }
    }

    /// Builds the index from descriptors in an existing synchronous registry.
    #[cfg(feature = "inventory")]
    pub(crate) fn from_descriptors(
        descriptors: impl IntoIterator<Item = ProviderDescriptor>,
    ) -> Self {
        let mut index = Self::default();
        for descriptor in descriptors {
            index.insert(&descriptor);
        }
        index
    }

    /// Resolves a selector to its canonical ID.
    pub(crate) fn canonical_id(&self, selector: &str) -> Option<String> {
        self.selectors.get(selector).cloned()
    }
}

/// Sync provider adapter that returns its captured descriptor without running
/// provider callbacks while the shared registration boundary is held.
pub(crate) struct CachedProvider<S: SyncServiceSpec> {
    provider: Arc<dyn ProviderDefinition<S>>,
    descriptor: ProviderDescriptor,
}

impl<S: SyncServiceSpec> CachedProvider<S> {
    pub(crate) fn new(
        provider: Arc<dyn ProviderDefinition<S>>,
        descriptor: ProviderDescriptor,
    ) -> Self {
        Self {
            provider,
            descriptor,
        }
    }
}

impl<S: SyncServiceSpec> ProviderMetadata for CachedProvider<S> {
    fn descriptor(&self) -> ProviderDescriptor {
        self.descriptor.clone()
    }
}

impl<S: SyncServiceSpec> ServiceProvider<S> for CachedProvider<S> {
    fn create_configured(
        &self,
        config: &S::Config,
    ) -> Result<S::Output, ProviderFailure<S::Error>> {
        self.provider.create_configured(config)
    }
}
