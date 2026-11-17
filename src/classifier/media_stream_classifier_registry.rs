// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Runtime Registry and global facade for media stream classifier providers.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::RwLock;

use qubit_spi::ProviderDefinition;
use qubit_spi::ProviderId;
use qubit_spi::ProviderRegistry;
use qubit_spi::ProviderSelection;
use qubit_spi::ProviderSelectionTargetRef;
use qubit_spi::ResolvingServiceProvider;
use qubit_spi::error::ProviderResolutionError;
use qubit_spi::error::RegistryMutationError;

use super::AsyncMediaStreamClassifierProvider;
use super::FfprobeCommandMediaStreamClassifierProvider;
use super::MediaStreamClassifierProvider;
use super::MediaStreamClassifierSpec;
use crate::MimeConfig;
use crate::MimeError;

/// Process-wide classifier Registry initialized with the built-in provider.
static GLOBAL_MEDIA_STREAM_CLASSIFIER_REGISTRY: LazyLock<MediaStreamClassifierRegistry> =
    LazyLock::new(MediaStreamClassifierRegistry::builtin);

/// Shared runtime Registry for media stream classifier definitions.
///
/// Clones observe the same synchronized provider catalog and default
/// selection. Use [`Self::global`] for App startup registrations intended for
/// independently developed downstream libraries.
///
/// # Examples
///
/// ```
/// use qubit_mime::MediaStreamClassifierRegistry;
/// let registry = MediaStreamClassifierRegistry::builtin();
/// assert!(!registry.provider_ids().is_empty());
/// ```
#[derive(Clone, Debug)]
pub struct MediaStreamClassifierRegistry {
    /// Typed provider Registry owning synchronized runtime state.
    providers: ProviderRegistry<MediaStreamClassifierSpec>,
    /// Explicit async classifier factories, indexed by canonical ID/alias.
    async_providers: Arc<RwLock<HashMap<String, Arc<dyn AsyncMediaStreamClassifierProvider>>>>,
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
        #[cfg(feature = "inventory")]
        let registry = Self {
            providers: super::media_stream_classifier_inventory::build_registry()
                .expect("submitted media stream classifier providers should register"),
            async_providers: Arc::new(RwLock::new(HashMap::new())),
        };
        #[cfg(not(feature = "inventory"))]
        let registry = Self::default();
        registry
            .register(FfprobeCommandMediaStreamClassifierProvider)
            .expect("built-in FFprobe classifier provider should register");
        registry
            .set_default_selection(
                ProviderSelection::named("ffprobe").expect("built-in FFprobe selection should be valid"),
            )
            .expect("built-in selection should be accepted");
        registry
            .register_async_provider(FfprobeCommandMediaStreamClassifierProvider)
            .expect("built-in FFprobe async provider should register");
        registry
    }

    /// Registers an explicit async classifier factory without constructing a
    /// classifier or starting its execution resources.
    ///
    /// # Type Parameters
    /// - `P`: Provider type that implements the asynchronous classifier
    ///   capability and remains owned by this registry.
    ///
    /// # Parameters
    /// - `provider`: Factory whose canonical ID and aliases become selectors.
    ///
    /// # Errors
    /// Returns an error when a canonical ID or alias is already registered.
    pub fn register_async_provider<P>(&self, provider: P) -> Result<(), String>
    where
        P: AsyncMediaStreamClassifierProvider + 'static,
    {
        let descriptor = provider.descriptor();
        let id = descriptor.id().as_str().to_owned();
        let provider: Arc<dyn AsyncMediaStreamClassifierProvider> = Arc::new(provider);
        let mut providers = self.async_providers.write().unwrap_or_else(|e| e.into_inner());
        let mut selectors = Vec::with_capacity(1 + descriptor.aliases().len());
        selectors.push(id.clone());
        selectors.extend(descriptor.aliases().iter().map(|alias| alias.as_str().to_owned()));
        for selector in &selectors {
            if let Some(existing) = providers.get(selector) {
                return Err(format!(
                    "async classifier selector `{selector}` is already owned by `{}` (attempted `{id}`)",
                    existing.descriptor().id().as_str()
                ));
            }
        }
        for selector in selectors {
            providers.insert(selector, Arc::clone(&provider));
        }
        Ok(())
    }

    /// Creates a classifier using an explicit async-capable provider choice.
    ///
    /// # Parameters
    /// - `selection`: Explicit provider selector or ordered fallback chain.
    /// - `config`: MIME settings passed to the selected provider.
    /// - `services`: Execution services used to construct the backend.
    ///
    /// # Returns
    /// The selected provider ID and its asynchronous classifier backend.
    ///
    /// # Errors
    /// Returns [`MimeError::AsyncUnsupported`] when no async-capable provider
    /// matches, or the provider error when backend construction fails.
    pub fn create_async_backend(
        &self,
        selection: &ProviderSelection,
        config: &MimeConfig,
        services: &crate::detector::MimeExecutionServices,
    ) -> Result<(String, Arc<dyn crate::AsyncMediaStreamClassifierBackend>), MimeError> {
        let selectors: Vec<&str> = match selection.target() {
            ProviderSelectionTargetRef::Auto => vec!["ffprobe"],
            ProviderSelectionTargetRef::Named(selector) => vec![selector.as_str()],
            ProviderSelectionTargetRef::Chain { selectors, .. } => {
                selectors.iter().map(|selector| selector.as_str()).collect()
            }
            _ => return Err(MimeError::AsyncUnsupported),
        };
        let providers = self.async_providers.read().unwrap_or_else(|e| e.into_inner());
        for selector in selectors {
            if let Some(provider) = providers.get(selector) {
                let id = provider.descriptor().id().as_str().to_owned();
                return Ok((id, provider.create_async_backend(config, services)?));
            }
        }
        Err(MimeError::AsyncUnsupported)
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

    /// Returns the selection used by future default resolutions.
    ///
    /// # Returns
    ///
    /// An owned snapshot independent from [`crate::MimeConfig`].
    #[inline]
    #[must_use]
    pub fn default_selection(&self) -> ProviderSelection {
        self.providers.default_selection()
    }

    /// Returns whether this registry is sealed.
    #[must_use]
    pub fn is_sealed(&self) -> bool {
        self.providers.is_sealed()
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

    /// Registers an owned self-described classifier provider.
    ///
    /// # Type Parameters
    /// - `P`: Concrete provider type implementing the classifier provider
    ///   contract.
    ///
    /// # Parameters
    ///
    /// * `provider` - Provider definition moved into Registry storage.
    ///
    /// # Errors
    ///
    /// Returns [`RegistryMutationError`] when its ID or an alias is already
    /// owned.
    #[inline]
    pub fn register<P>(&self, provider: P) -> Result<(), RegistryMutationError>
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
    /// Returns [`RegistryMutationError`] when its ID or an alias is already
    /// owned.
    #[inline]
    pub fn register_shared(
        &self,
        provider: Arc<dyn ProviderDefinition<MediaStreamClassifierSpec>>,
    ) -> Result<(), RegistryMutationError> {
        self.providers.register_shared(provider)
    }

    /// Replaces the selection used by future default resolutions.
    ///
    /// # Parameters
    ///
    /// * `selection` - Validated selection and creation fallback policy.
    #[inline]
    pub fn set_default_selection(&self, selection: ProviderSelection) -> Result<(), RegistryMutationError> {
        self.providers.set_default_selection(selection)
    }

    /// Seals this registry against further mutation.
    pub fn seal(&self) {
        self.providers.seal();
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
    #[inline]
    pub fn resolve_selected(
        &self,
        selection: &ProviderSelection,
    ) -> Result<ResolvingServiceProvider<MediaStreamClassifierSpec>, ProviderResolutionError> {
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
    #[inline]
    pub fn resolve(&self) -> Result<ResolvingServiceProvider<MediaStreamClassifierSpec>, ProviderResolutionError> {
        self.providers.resolve()
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
            async_providers: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use qubit_spi::ProviderSelection;

    use super::super::FfprobeCommandMediaStreamClassifierProvider;
    use super::MediaStreamClassifierRegistry;
    use crate::MimeConfig;
    use crate::MimeError;
    use crate::command_execution::FixedMimeExecutionPort;
    use crate::detector::MimeExecutionServices;

    #[test]
    fn test_async_registry_resolves_auto_alias_chain_and_unknown_selection() {
        let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
        let group = manager.group(qubit_operation::GroupOptions { capacity: 2 }).unwrap();
        let services =
            MimeExecutionServices::new(manager, group, Arc::new(FixedMimeExecutionPort::new(1, 2)), 32).unwrap();
        let registry = MediaStreamClassifierRegistry::builtin();
        let config = MimeConfig::default();

        for selection in [
            ProviderSelection::auto(),
            ProviderSelection::named("ffprobe-command").unwrap(),
            ProviderSelection::chain(["not-registered", "ffprobe-command"])
                .expect("the fallback selector chain should be valid"),
        ] {
            let (id, _backend) = registry
                .create_async_backend(&selection, &config, &services)
                .expect("the built-in async classifier should resolve its default and aliases");
            assert_eq!("ffprobe", id);
        }

        let error = registry
            .create_async_backend(&ProviderSelection::named("not-registered").unwrap(), &config, &services)
            .expect_err("unknown async selectors have no backend");
        assert!(matches!(error, MimeError::AsyncUnsupported));

        let empty = MediaStreamClassifierRegistry::default();
        assert!(matches!(
            empty.create_async_backend(&ProviderSelection::auto(), &config, &services),
            Err(MimeError::AsyncUnsupported)
        ));
    }

    #[test]
    fn test_async_registry_rejects_duplicate_provider_registration() {
        let registry = MediaStreamClassifierRegistry::builtin();
        let error = registry
            .register_async_provider(FfprobeCommandMediaStreamClassifierProvider)
            .expect_err("canonical selectors must have one owner");
        assert!(error.contains("ffprobe"));

        // The original alias still resolves after the rejected registration.
        let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
        let group = manager.group(qubit_operation::GroupOptions { capacity: 2 }).unwrap();
        let services =
            MimeExecutionServices::new(manager, group, Arc::new(FixedMimeExecutionPort::new(1, 2)), 32).unwrap();
        let (id, _backend) = registry
            .create_async_backend(
                &ProviderSelection::named("ffprobe-command").unwrap(),
                &MimeConfig::default(),
                &services,
            )
            .unwrap();
        assert_eq!("ffprobe", id);
    }
}
