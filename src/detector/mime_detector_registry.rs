// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Runtime Registry and process-wide facade for MIME detector providers.

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

use super::AsyncMimeDetectorProvider;
use super::FileCommandMimeDetectorProvider;
use super::MimeDetectorProvider;
use super::MimeDetectorSpec;
use super::RepositoryMimeDetectorProvider;

/// Process-wide MIME detector Registry initialized with built-in providers.
static GLOBAL_MIME_DETECTOR_REGISTRY: LazyLock<MimeDetectorRegistry> = LazyLock::new(MimeDetectorRegistry::builtin);

/// Async factories indexed by selector with their registration-time identity.
type AsyncProviderMap = HashMap<String, (String, Arc<dyn AsyncMimeDetectorProvider>)>;

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
        let registry = Self {
            providers: super::mime_detector_inventory::build_registry()
                .expect("submitted MIME detector providers should register"),
            async_providers: Arc::new(RwLock::new(HashMap::new())),
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
    /// Returns an error when the provider is unavailable or async unsupported.
    /// The selected provider is snapshotted before its factory runs, so factory
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
        let selectors: Vec<&str> = match selection.target() {
            ProviderSelectionTargetRef::Auto => vec!["repository"],
            ProviderSelectionTargetRef::Named(selector) => vec![selector.as_str()],
            ProviderSelectionTargetRef::Chain { selectors, .. } => {
                selectors.iter().map(|selector| selector.as_str()).collect()
            }
            _ => return Err(crate::MimeError::AsyncUnsupported),
        };
        let selected = {
            let providers = self.async_providers.read().unwrap_or_else(|e| e.into_inner());
            selectors.iter().find_map(|selector| providers.get(*selector).cloned())
        };
        if let Some((id, provider)) = selected {
            let backend = provider.create_async_backend_with_classifier(config, services, classifier.clone())?;
            return Ok((id, backend));
        }
        // Confirm a known synchronous selector is an unsupported capability,
        // while keeping unknown-selection diagnosis on the existing resolver.
        let _ = self.resolve_selected(selection);
        Err(crate::MimeError::AsyncUnsupported)
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
        let provider: Arc<dyn AsyncMimeDetectorProvider> = Arc::new(provider);
        let mut providers = self.async_providers.write().unwrap_or_else(|e| e.into_inner());
        if self.providers.is_sealed() {
            return Err(format!(
                "cannot register async MIME detector provider `{id}` after registry is sealed"
            ));
        }
        let mut selectors = Vec::with_capacity(1 + descriptor.aliases().len());
        selectors.push(id.clone());
        selectors.extend(descriptor.aliases().iter().map(|alias| alias.as_str().to_owned()));
        for selector in &selectors {
            if let Some(existing) = providers.get(selector) {
                return Err(format!(
                    "async provider selector `{selector}` is already owned by `{}` (attempted `{id}`)",
                    existing.0,
                ));
            }
        }
        for selector in selectors {
            providers.insert(selector, (id.clone(), Arc::clone(&provider)));
        }
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
    /// Returns [`RegistryMutationError`] when this Registry has been sealed.
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
        }
    }
}
