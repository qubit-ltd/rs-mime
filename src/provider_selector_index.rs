// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Shared selector identity checks for synchronous and asynchronous providers.

use std::collections::HashMap;

use qubit_spi::ProviderDescriptor;

/// Selector ownership shared by both provider capabilities.
#[derive(Clone, Debug, Default)]
pub(crate) struct ProviderSelectorIndex {
    /// Maps each registered provider selector to its canonical owner ID.
    selectors: HashMap<String, String>,
}

impl ProviderSelectorIndex {
    /// Rejects selectors already associated with another canonical ID.
    ///
    /// # Parameters
    /// - `descriptor`: Provider identity and aliases to validate.
    ///
    /// # Returns
    /// `Ok(())` when all selectors are unowned or belong to this provider;
    /// otherwise returns the conflicting selector and its canonical owner.
    pub(crate) fn validate(&self, descriptor: &ProviderDescriptor) -> Result<(), (String, String)> {
        let id = descriptor.id().as_str();
        for selector in std::iter::once(id).chain(descriptor.aliases().iter().map(|alias| alias.as_str())) {
            if let Some(owner) = self.selectors.get(selector)
                && owner != id
            {
                return Err((selector.to_owned(), owner.clone()));
            }
        }
        Ok(())
    }

    /// Adds selector ownership after registration succeeds.
    ///
    /// # Parameters
    /// - `descriptor`: Successfully registered provider identity and aliases.
    pub(crate) fn insert(&mut self, descriptor: &ProviderDescriptor) {
        let id = descriptor.id().as_str().to_owned();
        for selector in std::iter::once(id.as_str()).chain(descriptor.aliases().iter().map(|alias| alias.as_str())) {
            self.selectors.insert(selector.to_owned(), id.clone());
        }
    }

    /// Builds the index from descriptors in an existing synchronous registry.
    ///
    /// # Parameters
    /// - `descriptors`: Provider descriptors already registered.
    ///
    /// # Returns
    /// An index containing each descriptor's canonical ID and aliases.
    #[cfg(feature = "inventory")]
    pub(crate) fn from_descriptors(descriptors: impl IntoIterator<Item = ProviderDescriptor>) -> Self {
        let mut index = Self::default();
        for descriptor in descriptors {
            index.insert(&descriptor);
        }
        index
    }

    /// Resolves a selector to its canonical ID.
    ///
    /// # Parameters
    /// - `selector`: Provider ID or alias to resolve.
    ///
    /// # Returns
    /// The canonical provider ID, or `None` when the selector is unknown.
    pub(crate) fn canonical_id(&self, selector: &str) -> Option<String> {
        self.selectors.get(selector).cloned()
    }
}

mod cached_provider;

pub(crate) use cached_provider::CachedProvider;
