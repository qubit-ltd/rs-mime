// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Runtime Registry and global facade for media stream classifier providers.

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

use super::AsyncMediaStreamClassifierProvider;
use super::FfprobeCommandMediaStreamClassifierProvider;
use super::MediaStreamClassifierProvider;
use super::MediaStreamClassifierSpec;
use crate::MimeConfig;
use crate::MimeError;
use crate::async_selection::AsyncProviderCandidate;

/// Process-wide classifier Registry initialized with the built-in provider.
static GLOBAL_MEDIA_STREAM_CLASSIFIER_REGISTRY: LazyLock<MediaStreamClassifierRegistry> =
    LazyLock::new(MediaStreamClassifierRegistry::builtin);

/// Async factories indexed by selector with their registration-time identity.
type AsyncProviderMap = HashMap<String, AsyncProviderEntry>;

/// Async classifier factory and descriptor snapshot retained for its selectors.
#[derive(Clone, Debug)]
struct AsyncProviderEntry {
    id: String,
    priority: i32,
    is_builtin: bool,
    provider: Arc<dyn AsyncMediaStreamClassifierProvider>,
}

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
    async_providers: Arc<RwLock<AsyncProviderMap>>,
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
    ///
    /// # Panics
    ///
    /// Panics if the built-in provider cannot be registered or the built-in
    /// default selection is invalid.
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
    /// Returns [`MimeError::ProviderSelection`] when selectors cannot resolve,
    /// [`MimeError::AsyncUnsupported`] for a selected sync-only provider, or
    /// the final factory error after applying the selection's fallback policy.
    /// The candidates are snapshotted before factories run, so callbacks run
    /// without holding the async catalog lock.
    pub fn create_async_backend(
        &self,
        selection: &ProviderSelection,
        config: &MimeConfig,
        services: &crate::detector::MimeExecutionServices,
    ) -> Result<(String, Arc<dyn crate::AsyncMediaStreamClassifierBackend>), MimeError> {
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
            right_priority.cmp(left_priority).then_with(|| left_id.cmp(right_id))
        });
        let auto_builtin_ids = builtin_ids.into_iter().map(|(id, _)| id).collect();
        let mut auto_priority_ids = self
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
            |selector| {
                let selection = ProviderSelection::named(selector).ok()?;
                self.resolve_selected(&selection)
                    .ok()?
                    .candidate_descriptors()
                    .next()
                    .map(|descriptor| descriptor.id().as_str().to_owned())
            },
            auto_builtin_ids,
            auto_priority_ids,
            "media stream classifier",
        )?;

        let mut last_error = None;
        for candidate in candidates {
            let (id, provider) = match candidate {
                AsyncProviderCandidate::Async { id, provider } => (id, Some(provider)),
                AsyncProviderCandidate::SyncOnly { id } => (id, None),
            };
            let result = provider.map_or_else(
                || Err(MimeError::AsyncUnsupported),
                |entry| {
                    entry
                        .provider
                        .create_async_backend(config, services)
                        .map(|backend| (id, backend))
                },
            );
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
            MimeError::ProviderSelection("no media stream classifier candidates match the selection".to_owned())
        }))
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
    #[inline]
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

    /// Lists canonical IDs with registered asynchronous classifier factories.
    ///
    /// Alias selectors are omitted and every ID is returned once.
    #[must_use]
    pub fn async_provider_ids(&self) -> Vec<ProviderId> {
        let providers = self.async_providers.read().unwrap_or_else(|e| e.into_inner());
        let mut ids = providers.values().map(|entry| entry.id.clone()).collect::<Vec<_>>();
        ids.sort();
        ids.dedup();
        ids.into_iter().filter_map(|id| ProviderId::new(&id).ok()).collect()
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
    /// # Returns
    ///
    /// `Ok(())` after the provider and its aliases are registered.
    ///
    /// # Errors
    /// Returns an error when the registry is sealed or a canonical ID or alias
    /// is already registered. Register all providers before sealing.
    pub fn register_async_provider<P>(&self, provider: P) -> Result<(), String>
    where
        P: AsyncMediaStreamClassifierProvider + 'static,
    {
        let descriptor = provider.descriptor();
        let id = descriptor.id().as_str().to_owned();
        let priority = descriptor.priority();
        let is_builtin = TypeId::of::<P>() == TypeId::of::<FfprobeCommandMediaStreamClassifierProvider>();
        let provider: Arc<dyn AsyncMediaStreamClassifierProvider> = Arc::new(provider);
        let mut providers = self.async_providers.write().unwrap_or_else(|e| e.into_inner());
        if self.providers.is_sealed() {
            return Err(format!(
                "cannot register async media classifier `{id}` after registry is sealed"
            ));
        }
        let mut selectors = Vec::with_capacity(1 + descriptor.aliases().len());
        selectors.push(id.clone());
        selectors.extend(descriptor.aliases().iter().map(|alias| alias.as_str().to_owned()));
        for selector in &selectors {
            if let Some(existing) = providers.get(selector) {
                return Err(format!(
                    "async classifier selector `{selector}` is already owned by `{}` (attempted `{id}`)",
                    existing.id
                ));
            }
        }
        for selector in selectors {
            providers.insert(
                selector,
                AsyncProviderEntry {
                    id: id.clone(),
                    priority,
                    is_builtin,
                    provider: Arc::clone(&provider),
                },
            );
        }
        Ok(())
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
    /// # Returns
    ///
    /// `Ok(())` after the provider is registered.
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
    /// # Returns
    ///
    /// `Ok(())` after the shared provider is registered.
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
    ///
    /// # Returns
    ///
    /// `Ok(())` after the default selection is replaced.
    ///
    /// # Errors
    ///
    /// Returns [`RegistryMutationError::Sealed`] when the registry rejects
    /// further configuration changes.
    #[inline]
    pub fn set_default_selection(&self, selection: ProviderSelection) -> Result<(), RegistryMutationError> {
        self.providers.set_default_selection(selection)
    }

    /// Seals this registry against further mutation.
    #[inline]
    pub fn seal(&self) {
        let _async_providers = self.async_providers.write().unwrap_or_else(|e| e.into_inner());
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
