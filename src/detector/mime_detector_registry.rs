// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Runtime Registry and process-wide facade for MIME detector providers.

use std::any::TypeId;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::RwLock;

use qubit_spi::ProviderDefinition;
use qubit_spi::ProviderId;
use qubit_spi::ProviderRegistry;
use qubit_spi::ProviderSelection;
use qubit_spi::ResolvingServiceProvider;
use qubit_spi::error::ProviderResolutionError;
use qubit_spi::error::RegistryMutationError;

use super::AsyncMimeDetectorProvider;
use super::FileCommandMimeDetectorProvider;
use super::MimeDetectorProvider;
use super::MimeDetectorSpec;
use super::RepositoryMimeDetectorProvider;
use super::internal::async_provider_entry::AsyncBackendSelection;
use super::internal::async_provider_entry::AsyncProviderEntry;
use crate::async_selection::AsyncProviderCandidate;
use crate::provider_selector_index::CachedProvider;
use crate::provider_selector_index::ProviderSelectorIndex;

/// Process-wide MIME detector Registry initialized with built-in providers.
static GLOBAL_MIME_DETECTOR_REGISTRY: LazyLock<MimeDetectorRegistry> = LazyLock::new(MimeDetectorRegistry::builtin);

/// Async factories indexed by selector with their registration-time identity.
type AsyncProviderMap = HashMap<String, AsyncProviderEntry>;

/// Returns the stable built-in order used by automatic MIME selection.
///
/// # Parameters
/// - `id`: Canonical built-in detector ID.
///
/// # Returns
/// A lower rank for built-ins that Auto selection must try earlier.
fn builtin_auto_rank(id: &str) -> usize {
    match id {
        "repository" => 0,
        "file" => 1,
        _ => 2,
    }
}

/// Shared runtime Registry for MIME detector provider definitions.
///
/// Clones observe the same underlying provider catalog and default selection.
/// Use [`Self::global`] when App startup registrations must be visible to
/// independently developed downstream libraries.
///
/// # Examples
///
/// ```
/// use qubit_mime::MimeDetectorRegistry;
/// let registry = MimeDetectorRegistry::builtin();
/// assert!(!registry.provider_ids().is_empty());
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct MimeDetectorRegistry {
    /// Typed provider Registry owning synchronized runtime state.
    providers: ProviderRegistry<MimeDetectorSpec>,
    /// Explicit operation-capable factories, indexed by canonical ID/alias.
    async_providers: Arc<RwLock<AsyncProviderMap>>,
    /// Selector ownership shared across sync and async capabilities.
    selector_ids: Arc<RwLock<ProviderSelectorIndex>>,
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
    pub fn builtin() -> Self {
        #[cfg(feature = "inventory")]
        let registry = {
            let providers = super::mime_detector_inventory::build_registry()
                .expect("submitted MIME detector providers should register");
            let selector_ids = ProviderSelectorIndex::from_descriptors(providers.descriptors());
            Self {
                providers,
                async_providers: Arc::new(RwLock::new(HashMap::new())),
                selector_ids: Arc::new(RwLock::new(selector_ids)),
            }
        };
        #[cfg(not(feature = "inventory"))]
        let registry = Self::default();
        registry
            .register(RepositoryMimeDetectorProvider)
            .expect("built-in repository MIME provider should register");
        registry
            .register(FileCommandMimeDetectorProvider)
            .expect("built-in file MIME provider should register");
        registry
            .set_default_selection(
                ProviderSelection::named("repository").expect("built-in repository selection should be valid"),
            )
            .expect("built-in selection should be accepted");
        registry
            .register_async_provider(RepositoryMimeDetectorProvider)
            .expect("built-in repository async provider should register");
        registry
            .register_async_provider(FileCommandMimeDetectorProvider)
            .expect("built-in file async provider should register");
        registry
    }

    /// Resolves an operation-capable detector backend from an explicit
    /// selection. A synchronous-only provider selected from the sync registry
    /// produces `AsyncUnsupported` and its synchronous factory is not invoked.
    ///
    /// # Parameters
    /// - `selection`: Provider selector or selector chain to resolve.
    /// - `config`: MIME detection configuration.
    /// - `services`: Runtime execution services supplied to the provider.
    ///
    /// # Returns
    /// The selected provider ID and its async detector backend.
    ///
    /// # Errors
    /// Returns [`crate::MimeError::ProviderSelection`] when selectors cannot
    /// resolve, [`crate::MimeError::AsyncUnsupported`] for a selected sync-only
    /// provider, or the final factory error after applying the selection's
    /// fallback policy. Candidates are snapshotted before factories run, so
    /// callbacks run without holding the async catalog lock.
    pub fn create_async_backend(
        &self,
        selection: &ProviderSelection,
        config: &crate::MimeConfig,
        services: &crate::MimeExecutionServices,
    ) -> Result<(String, Arc<dyn crate::AsyncMimeDetectorBackend>), crate::MimeError> {
        self.create_async_backend_with_classifier(selection, config, services, None)
    }

    /// Resolves an async detector and attaches an optional async stream
    /// classifier to providers that support operation-backed refinement.
    ///
    /// # Parameters
    /// - `selection`: Provider selector or selector chain to resolve.
    /// - `config`: MIME detection configuration.
    /// - `services`: Runtime execution services supplied to the provider.
    /// - `classifier`: Optional classifier attached to supporting providers.
    ///
    /// # Returns
    /// The selected provider ID and its async detector backend.
    ///
    /// # Errors
    /// Returns an error when the provider is unavailable or async unsupported.
    pub fn create_async_backend_with_classifier(
        &self,
        selection: &ProviderSelection,
        config: &crate::MimeConfig,
        services: &crate::MimeExecutionServices,
        classifier: Option<Arc<dyn crate::AsyncMediaStreamClassifierBackend>>,
    ) -> Result<(String, Arc<dyn crate::AsyncMimeDetectorBackend>), crate::MimeError> {
        self.create_async_backend_internal(selection, config, services, classifier)
            .map(|(id, backend, _, _)| (id, backend))
    }

    /// Creates an async backend and retains the trusted repository belonging
    /// to the exact built-in provider, if that provider was selected.
    ///
    /// # Parameters
    /// - `selection`: Provider selector or selector chain to resolve.
    /// - `config`: MIME detection configuration.
    /// - `services`: Runtime execution services supplied to the provider.
    /// - `classifier`: Optional async classifier attached to supporting
    ///   providers.
    ///
    /// # Returns
    /// The selected provider ID, backend, and trusted bundled repository when
    /// the exact built-in repository provider was selected.
    ///
    /// # Errors
    /// Returns an error when the provider is unavailable or async unsupported.
    pub(crate) fn create_async_backend_internal(
        &self,
        selection: &ProviderSelection,
        config: &crate::MimeConfig,
        services: &crate::MimeExecutionServices,
        classifier: Option<Arc<dyn crate::AsyncMediaStreamClassifierBackend>>,
    ) -> Result<AsyncBackendSelection, crate::MimeError> {
        let selector_ids = self.selector_ids.read().unwrap_or_else(|e| e.into_inner());
        let async_catalog = {
            let providers = self.async_providers.read().unwrap_or_else(|e| e.into_inner());
            providers
                .iter()
                .map(|(selector, entry)| (selector.clone(), (entry.id.clone(), entry.priority, entry.clone())))
                .collect::<HashMap<_, _>>()
        };
        let mut builtin_ids = async_catalog
            .values()
            .filter(|(_, _, entry)| entry.is_builtin)
            .map(|(id, priority, _)| (id.clone(), *priority))
            .collect::<Vec<_>>();
        builtin_ids.sort_by(|(left_id, left_priority), (right_id, right_priority)| {
            builtin_auto_rank(left_id)
                .cmp(&builtin_auto_rank(right_id))
                .then_with(|| right_priority.cmp(left_priority))
                .then_with(|| left_id.cmp(right_id))
        });
        let auto_builtin_ids = builtin_ids.into_iter().map(|(id, _)| id).collect();
        let mut auto_priority_ids = self
            .providers
            .resolve_selected(&ProviderSelection::auto())
            .map(|resolved| {
                resolved
                    .candidate_descriptors()
                    .map(|descriptor| (descriptor.id().as_str().to_owned(), descriptor.priority()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        auto_priority_ids.extend(async_catalog.values().map(|(id, priority, _)| (id.clone(), *priority)));
        let candidates = crate::async_selection::resolve(
            selection,
            &async_catalog,
            |selector| selector_ids.canonical_id(selector),
            auto_builtin_ids,
            auto_priority_ids,
            "MIME detector",
        )?;
        drop(selector_ids);

        let mut last_error = None;
        for candidate in candidates {
            let (id, provider) = match candidate {
                AsyncProviderCandidate::Async { id, provider } => (id, Some(provider)),
                AsyncProviderCandidate::SyncOnly { id } => (id, None),
            };
            let result = if let Some(entry) = provider {
                entry
                    .provider
                    .create_async_backend_with_classifier(config, services, classifier.clone())
                    .map(|backend| {
                        let repository = entry
                            .uses_bundled_repository
                            .then(crate::MimeRepository::bundled_shared);
                        (id, backend, repository, entry.self_metered_provider_reads)
                    })
            } else {
                Err(crate::MimeError::AsyncUnsupported)
            };
            match result {
                Ok(selection) => return Ok(selection),
                Err(error) => {
                    if !crate::async_selection::should_fallback(selection.fallback_policy(), &error) {
                        return Err(error);
                    }
                    last_error = Some(error);
                }
            }
        }
        Err(last_error.unwrap_or_else(|| {
            crate::MimeError::ProviderSelection("no MIME detector candidates match the selection".to_owned())
        }))
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
    pub fn global() -> &'static Self {
        &GLOBAL_MIME_DETECTOR_REGISTRY
    }

    /// Returns the selection used by future default resolutions.
    ///
    /// # Returns
    ///
    /// An owned snapshot independent from any [`crate::MimeConfig`].
    #[inline]
    #[must_use]
    pub fn default_selection(&self) -> ProviderSelection {
        self.providers.default_selection()
    }

    /// Returns whether this registry is sealed.
    #[inline]
    #[must_use]
    pub fn is_sealed(&self) -> bool {
        let _snapshot = self.selector_ids.read().unwrap_or_else(|e| e.into_inner());
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
        let _snapshot = self.selector_ids.read().unwrap_or_else(|e| e.into_inner());
        self.providers.provider_ids()
    }

    /// Lists canonical IDs with registered asynchronous MIME detector
    /// factories.
    ///
    /// Alias selectors are omitted and every ID is returned once.
    #[must_use]
    pub fn async_provider_ids(&self) -> Vec<ProviderId> {
        let _snapshot = self.selector_ids.read().unwrap_or_else(|e| e.into_inner());
        let providers = self.async_providers.read().unwrap_or_else(|e| e.into_inner());
        let mut ids = providers.values().map(|entry| entry.id.clone()).collect::<Vec<_>>();
        ids.sort();
        ids.dedup();
        ids.into_iter().filter_map(|id| ProviderId::new(&id).ok()).collect()
    }

    /// Registers explicit async construction for a provider already available
    /// through the synchronous catalog. This registration performs no service
    /// creation, I/O, or executor startup.
    ///
    /// # Parameters
    /// - `provider`: Async-capable provider whose selector IDs are registered.
    ///
    /// # Type Parameters
    /// - `P`: Concrete async provider type.
    ///
    /// # Returns
    /// `Ok(())` when the provider and its aliases are registered.
    ///
    /// # Errors
    /// Returns an error if the registry is sealed or any selector is already
    /// owned by another provider. Register all providers before sealing.
    pub fn register_async_provider<P>(&self, provider: P) -> Result<(), String>
    where
        P: AsyncMimeDetectorProvider + 'static,
    {
        let descriptor = provider.descriptor();
        let id = descriptor.id().as_str().to_owned();
        let uses_bundled_repository = TypeId::of::<P>() == TypeId::of::<RepositoryMimeDetectorProvider>();
        let self_metered_provider_reads = TypeId::of::<P>() == TypeId::of::<FileCommandMimeDetectorProvider>();
        let is_builtin =
            uses_bundled_repository || TypeId::of::<P>() == TypeId::of::<FileCommandMimeDetectorProvider>();
        let provider: Arc<dyn AsyncMimeDetectorProvider> = Arc::new(provider);
        let mut selector_ids = self.selector_ids.write().unwrap_or_else(|e| e.into_inner());
        let mut providers = self.async_providers.write().unwrap_or_else(|e| e.into_inner());
        if self.providers.is_sealed() {
            return Err(format!(
                "cannot register async MIME detector provider `{id}` after registry is sealed"
            ));
        }
        let mut selectors = Vec::with_capacity(1 + descriptor.aliases().len());
        selectors.push(id.clone());
        selectors.extend(descriptor.aliases().iter().map(|alias| alias.as_str().to_owned()));
        if let Err((selector, owner)) = selector_ids.validate(&descriptor) {
            return Err(format!(
                "provider selector `{selector}` is already owned by `{owner}` (attempted `{id}`)"
            ));
        }
        for selector in &selectors {
            if let Some(existing) = providers.get(selector) {
                return Err(format!(
                    "async provider selector `{selector}` is already owned by `{}` (attempted `{id}`)",
                    existing.id,
                ));
            }
        }
        for selector in selectors {
            providers.insert(
                selector,
                AsyncProviderEntry {
                    id: id.clone(),
                    priority: descriptor.priority(),
                    is_builtin,
                    provider: Arc::clone(&provider),
                    uses_bundled_repository,
                    self_metered_provider_reads,
                },
            );
        }
        selector_ids.insert(&descriptor);
        Ok(())
    }

    /// Registers an owned self-described detector provider.
    ///
    /// # Parameters
    ///
    /// * `provider` - Provider definition moved into shared Registry storage.
    ///
    /// # Type Parameters
    ///
    /// * `P` - Owned provider definition registered in this Registry.
    ///
    /// # Returns
    ///
    /// `Ok(())` after the provider is registered.
    ///
    /// # Errors
    ///
    /// Returns [`RegistryMutationError`] when the provider ID or an alias is
    /// already owned or when the Registry has been sealed. The Registry
    /// remains unchanged on error.
    #[inline]
    pub fn register<P>(&self, provider: P) -> Result<(), RegistryMutationError>
    where
        P: MimeDetectorProvider,
    {
        if self.providers.is_sealed() {
            return Err(RegistryMutationError::Sealed);
        }
        let provider: Arc<dyn ProviderDefinition<MimeDetectorSpec>> = Arc::new(provider);
        let descriptor = provider.descriptor();
        let mut selector_ids = self.selector_ids.write().unwrap_or_else(|e| e.into_inner());
        selector_ids
            .validate(&descriptor)
            .map_err(|(selector, owner)| RegistryMutationError::DuplicateSelector {
                selector: selector.into_boxed_str(),
                existing_provider: owner.into_boxed_str(),
                provider: descriptor.id().as_str().into(),
            })?;
        self.providers
            .register_shared(Arc::new(CachedProvider::new(provider, descriptor.clone())))?;
        selector_ids.insert(&descriptor);
        Ok(())
    }

    /// Registers an already shared self-described detector provider.
    ///
    /// # Parameters
    ///
    /// * `provider` - Type-erased shared provider definition retained by the
    ///   Registry. Concrete owned providers normally use [`Self::register`]
    ///   through the domain-specific [`MimeDetectorProvider`] contract.
    ///
    /// # Returns
    ///
    /// `Ok(())` after the provider is registered.
    ///
    /// # Errors
    ///
    /// Returns [`RegistryMutationError`] when the provider ID or an alias is
    /// already owned or when the Registry has been sealed. The Registry
    /// remains unchanged on error.
    #[inline]
    pub fn register_shared(
        &self,
        provider: Arc<dyn ProviderDefinition<MimeDetectorSpec>>,
    ) -> Result<(), RegistryMutationError> {
        if self.providers.is_sealed() {
            return Err(RegistryMutationError::Sealed);
        }
        let descriptor = provider.descriptor();
        let mut selector_ids = self.selector_ids.write().unwrap_or_else(|e| e.into_inner());
        selector_ids
            .validate(&descriptor)
            .map_err(|(selector, owner)| RegistryMutationError::DuplicateSelector {
                selector: selector.into_boxed_str(),
                existing_provider: owner.into_boxed_str(),
                provider: descriptor.id().as_str().into(),
            })?;
        self.providers
            .register_shared(Arc::new(CachedProvider::new(provider, descriptor.clone())))?;
        selector_ids.insert(&descriptor);
        Ok(())
    }

    /// Replaces the selection used by future default resolutions.
    ///
    /// # Parameters
    ///
    /// * `selection` - Validated selection and creation fallback policy.
    ///
    /// # Returns
    ///
    /// `Ok(())` after the default selection is replaced.
    ///
    /// # Errors
    ///
    /// Returns [`RegistryMutationError`] when this Registry has been sealed.
    #[inline]
    pub fn set_default_selection(&self, selection: ProviderSelection) -> Result<(), RegistryMutationError> {
        self.providers.set_default_selection(selection)
    }

    /// Seals this registry against further mutation.
    #[inline]
    pub fn seal(&self) {
        let _boundary = self.selector_ids.write().unwrap_or_else(|e| e.into_inner());
        let _async_providers = self.async_providers.write().unwrap_or_else(|e| e.into_inner());
        self.providers.seal();
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
    /// A point-in-time candidate snapshot with inherent creation methods.
    ///
    /// # Errors
    ///
    /// Returns [`ProviderResolutionError`] when the selection matches no
    /// registered provider.
    #[inline]
    pub fn resolve_selected(
        &self,
        selection: &ProviderSelection,
    ) -> Result<ResolvingServiceProvider<MimeDetectorSpec>, ProviderResolutionError> {
        let _snapshot = self.selector_ids.read().unwrap_or_else(|e| e.into_inner());
        self.providers.resolve_selected(selection)
    }

    /// Resolves the Registry's current default selection.
    ///
    /// This stage does not create a detector or inspect service configuration.
    ///
    /// # Returns
    ///
    /// A point-in-time candidate snapshot with inherent creation methods.
    ///
    /// # Errors
    ///
    /// Returns [`ProviderResolutionError`] when the stored default matches no
    /// registered provider.
    #[inline]
    pub fn resolve(&self) -> Result<ResolvingServiceProvider<MimeDetectorSpec>, ProviderResolutionError> {
        let _snapshot = self.selector_ids.read().unwrap_or_else(|e| e.into_inner());
        self.providers.resolve()
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
            async_providers: Arc::new(RwLock::new(HashMap::new())),
            selector_ids: Arc::new(RwLock::new(ProviderSelectorIndex::default())),
        }
    }
}

#[cfg(test)]
mod async_catalog_recovery_tests {
    use std::panic::AssertUnwindSafe;
    use std::panic::catch_unwind;
    use std::sync::Arc;

    use qubit_operation::GroupOptions;
    use qubit_operation::OperationManager;
    use qubit_spi::ProviderSelection;
    use qubit_spi::ProviderDescriptor;
    use qubit_spi::ProviderId;
    use qubit_spi::ProviderMetadata;

    use super::AsyncMimeDetectorProvider;
    use super::FileCommandMimeDetectorProvider;
    use super::MimeDetectorRegistry;
    use crate::FixedMimeExecutionPort;
    use crate::MimeConfig;
    use crate::MimeExecutionServices;

    #[derive(Debug)]
    struct CatalogRecoveryProvider;

    impl ProviderMetadata for CatalogRecoveryProvider {
        fn descriptor(&self) -> ProviderDescriptor {
            ProviderDescriptor::new(ProviderId::new("catalog-recovery").expect("valid provider ID"))
        }
    }

    impl AsyncMimeDetectorProvider for CatalogRecoveryProvider {
        fn create_async_backend(
            &self,
            _config: &MimeConfig,
            _services: &MimeExecutionServices,
        ) -> Result<Arc<dyn crate::AsyncMimeDetectorBackend>, crate::MimeError> {
            Err(crate::MimeError::AsyncUnsupported)
        }
    }

    #[test]
    fn test_poisoned_async_catalog_recovers_for_selection_registration_and_sealing() {
        let registry = MimeDetectorRegistry::builtin();
        let catalog_poisoned = catch_unwind(AssertUnwindSafe(|| {
            let _guard = registry.async_providers.write().expect("catalog should be unpoisoned");
            panic!("poison detector async catalog");
        }));
        assert!(catalog_poisoned.is_err());
        let selector_index_poisoned = catch_unwind(AssertUnwindSafe(|| {
            let _guard = registry.selector_ids.write().expect("selector index should be unpoisoned");
            panic!("poison detector selector index");
        }));
        assert!(selector_index_poisoned.is_err());

        let manager = OperationManager::new(Default::default()).expect("operation manager should be created");
        let group = manager
            .group(GroupOptions { capacity: 1 })
            .expect("operation group should be created");
        let services = MimeExecutionServices::new(manager, group, Arc::new(FixedMimeExecutionPort::new(1, 1)), 32)
            .expect("execution services should be created");
        let selection = ProviderSelection::named("repository").expect("built-in selector should be valid");
        let (id, _) = registry
            .create_async_backend(&selection, &MimeConfig::default(), &services)
            .expect("selection should recover the poisoned selector index");
        assert_eq!("repository", id);
        registry
            .register_async_provider(CatalogRecoveryProvider)
            .expect("catalog should accept a provider after poison recovery");
        registry.seal();
        assert!(registry.is_sealed());
        let async_ids = registry.async_provider_ids();
        assert!(async_ids.iter().any(|id| id.as_str() == "repository"));
        assert!(async_ids.iter().any(|id| id.as_str() == "catalog-recovery"));
        assert!(registry.register_async_provider(FileCommandMimeDetectorProvider).is_err());
    }
}
