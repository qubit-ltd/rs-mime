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
pub struct MimeDetectorRegistry {
    /// Typed provider Registry owning synchronized runtime state.
    providers: ProviderRegistry<MimeDetectorSpec>,
    /// Explicit operation-capable factories, indexed by canonical ID/alias.
    async_providers: Arc<RwLock<HashMap<String, Arc<dyn AsyncMimeDetectorProvider>>>>,
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
    /// Returns an error if any selector is already owned by another provider.
    pub fn register_async_provider<P>(&self, provider: P) -> Result<(), String>
    where
        P: AsyncMimeDetectorProvider + 'static,
    {
        let descriptor = provider.descriptor();
        let id = descriptor.id().as_str().to_owned();
        let provider: Arc<dyn AsyncMimeDetectorProvider> = Arc::new(provider);
        let mut providers = self.async_providers.write().unwrap_or_else(|e| e.into_inner());
        let mut selectors = Vec::with_capacity(1 + descriptor.aliases().len());
        selectors.push(id.clone());
        selectors.extend(descriptor.aliases().iter().map(|alias| alias.as_str().to_owned()));
        for selector in &selectors {
            if let Some(existing) = providers.get(selector) {
                return Err(format!(
                    "async provider selector `{selector}` is already owned by `{}` (attempted `{id}`)",
                    existing.descriptor().id().as_str(),
                ));
            }
        }
        for selector in selectors {
            providers.insert(selector, Arc::clone(&provider));
        }
        Ok(())
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
        let providers = self.async_providers.read().unwrap_or_else(|e| e.into_inner());
        for selector in selectors {
            let Some(provider) = providers.get(selector) else {
                continue;
            };
            let id = provider.descriptor().id().as_str().to_owned();
            let backend = provider.create_async_backend_with_classifier(config, services, classifier.clone())?;
            return Ok((id, backend));
        }
        drop(providers);
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
        self.providers.seal();
    }

    /// Returns whether this registry is sealed.
    #[inline]
    #[must_use]
    pub fn is_sealed(&self) -> bool {
        self.providers.is_sealed()
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
            async_providers: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use qubit_spi::ProviderDefinition;
    use qubit_spi::ProviderSelection;

    use super::MimeDetectorRegistry;
    use super::MimeDetectorSpec;
    use super::RepositoryMimeDetectorProvider;

    fn services() -> crate::MimeExecutionServices {
        let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
        let group = manager.group(qubit_operation::GroupOptions { capacity: 1 }).unwrap();
        crate::MimeExecutionServices::new(manager, group, Arc::new(crate::FixedMimeExecutionPort::new(1, 1)), 1024)
            .unwrap()
    }

    #[test]
    fn test_shared_provider_registration_drives_default_resolution() {
        let registry = MimeDetectorRegistry::default();
        let shared: Arc<dyn ProviderDefinition<MimeDetectorSpec>> = Arc::new(RepositoryMimeDetectorProvider);
        registry
            .register_shared(shared)
            .expect("shared provider should register in an empty registry");
        registry
            .set_default_selection(ProviderSelection::named("repository").unwrap())
            .expect("registered provider should be selectable as the default");

        let selection = registry.default_selection();
        assert_eq!(ProviderSelection::named("repository").unwrap(), selection);
        assert_eq!("repository", registry.provider_ids()[0].as_str());
        let detector = registry
            .resolve()
            .expect("stored default should resolve")
            .create()
            .expect("shared repository provider should create a detector");
        assert_eq!(
            Some("application/pdf".to_owned()),
            detector
                .detect_by_filename("document.pdf")
                .expect("resolved detector should serve filename requests")
        );

        let clone = registry.clone();
        assert_eq!(registry.provider_ids(), clone.provider_ids());
        assert_eq!(registry.default_selection(), clone.default_selection());
        registry.seal();
        assert!(registry.is_sealed());
    }

    #[test]
    fn test_default_registry_starts_empty_and_auto_selection_is_unresolved() {
        let registry = MimeDetectorRegistry::default();

        assert!(registry.provider_ids().is_empty());
        assert!(registry.resolve().is_err());
        assert!(!registry.is_sealed());
        assert_eq!(ProviderSelection::auto(), registry.default_selection());

        registry
            .register(RepositoryMimeDetectorProvider)
            .expect("an empty default registry should accept its first provider");
        let detector = registry
            .resolve()
            .expect("auto selection should resolve after registration")
            .create()
            .expect("registered repository provider should construct");
        assert_eq!(
            Some("application/pdf".to_owned()),
            detector.detect_by_filename("document.pdf").unwrap()
        );
    }

    #[test]
    fn test_global_registry_returns_process_stable_facade() {
        let first = MimeDetectorRegistry::global();
        let second = MimeDetectorRegistry::global();
        assert!(std::ptr::eq(first, second));
        assert!(first.provider_ids().iter().any(|id| id.as_str() == "repository"));
    }

    #[test]
    fn test_builtin_async_providers_create_backends_without_spawning_commands() {
        let registry = MimeDetectorRegistry::builtin();
        let services = services();
        let config = crate::MimeConfig::default();

        let (auto_id, auto_backend) = registry
            .create_async_backend(&ProviderSelection::auto(), &config, &services)
            .expect("automatic async selection should use the stable repository default");
        assert_eq!("repository", auto_id);
        assert!(matches!(
            auto_backend.content_requirement(),
            crate::ContentRequirement::Prefix(_)
        ));

        let (repository_id, repository_backend) = registry
            .create_async_backend(&ProviderSelection::named("repository").unwrap(), &config, &services)
            .expect("repository async provider should construct without external services");
        assert_eq!("repository", repository_id);
        assert!(matches!(
            repository_backend.content_requirement(),
            crate::ContentRequirement::Prefix(_)
        ));

        let (file_id, file_backend) = registry
            .create_async_backend(&ProviderSelection::named("file").unwrap(), &config, &services)
            .expect("file async backend construction should not probe the command");
        assert_eq!("file", file_id);
        assert_eq!(crate::ContentRequirement::Complete, file_backend.content_requirement());
    }

    #[test]
    fn test_async_provider_chain_skips_unknown_selector_and_uses_first_registered_backend() {
        let registry = MimeDetectorRegistry::builtin();
        let selection = ProviderSelection::chain(["not-registered", "file", "repository"])
            .expect("chain selectors should be valid");

        let (id, backend) = registry
            .create_async_backend(&selection, &crate::MimeConfig::default(), &services())
            .expect("chain should continue to the first available async provider");

        assert_eq!("file", id);
        assert_eq!(crate::ContentRequirement::Complete, backend.content_requirement());
    }

    #[test]
    fn test_duplicate_shared_provider_registration_is_atomic() {
        let registry = MimeDetectorRegistry::default();
        let provider: Arc<dyn ProviderDefinition<MimeDetectorSpec>> = Arc::new(RepositoryMimeDetectorProvider);
        registry
            .register_shared(provider)
            .expect("first shared provider registration should succeed");
        let duplicate: Arc<dyn ProviderDefinition<MimeDetectorSpec>> = Arc::new(RepositoryMimeDetectorProvider);

        assert!(registry.register_shared(duplicate).is_err());
        assert_eq!(1, registry.provider_ids().len());
        assert_eq!("repository", registry.provider_ids()[0].as_str());
    }

    #[test]
    fn test_registry_selection_errors_and_seal_preserve_catalog_state() {
        let registry = MimeDetectorRegistry::builtin();
        let missing = ProviderSelection::named("missing-provider").unwrap();
        registry
            .set_default_selection(missing.clone())
            .expect("selection mutation is separate from provider resolution");
        assert_eq!(missing, registry.default_selection());
        assert!(registry.resolve().is_err());
        assert!(registry.resolve_selected(&missing).is_err());
        registry
            .set_default_selection(ProviderSelection::named("repository").unwrap())
            .unwrap();
        registry.seal();
        assert!(registry.is_sealed());
        assert!(
            registry
                .set_default_selection(ProviderSelection::named("file").unwrap())
                .is_err()
        );
        assert!(registry.register(RepositoryMimeDetectorProvider).is_err());
        assert_eq!(2, registry.provider_ids().len());
    }

    #[test]
    fn test_duplicate_async_provider_selector_is_rejected_without_replacing_builtin() {
        let registry = MimeDetectorRegistry::builtin();
        assert!(
            registry
                .register_async_provider(RepositoryMimeDetectorProvider)
                .is_err()
        );
        let (id, backend) = registry
            .create_async_backend(
                &ProviderSelection::named("repository").unwrap(),
                &crate::MimeConfig::default(),
                &services(),
            )
            .unwrap();
        assert_eq!("repository", id);
        assert!(matches!(
            backend.content_requirement(),
            crate::ContentRequirement::Prefix(_)
        ));
    }

    #[test]
    fn test_async_resolution_of_unknown_selector_returns_unsupported() {
        let registry = MimeDetectorRegistry::builtin();
        let services = services();

        assert!(matches!(
            registry.create_async_backend(
                &ProviderSelection::named("not-registered").unwrap(),
                &crate::MimeConfig::default(),
                &services
            ),
            Err(crate::MimeError::AsyncUnsupported)
        ));
    }
}
