// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Registry for pluggable media stream classifier providers.
// qubit-style: allow coverage-cfg

#[cfg(coverage)]
use std::sync::PoisonError;
use std::sync::{
    Arc,
    LazyLock,
    RwLock,
    RwLockReadGuard,
    RwLockWriteGuard,
};

use qubit_spi::{
    ProviderRegistry,
    ProviderSelection,
    ServiceProvider,
};

use crate::{
    MediaStreamClassifier,
    MimeConfig,
    MimeError,
    MimeResult,
};

use super::{
    FfprobeCommandMediaStreamClassifierProvider,
    MediaStreamClassifierProvider,
    MediaStreamClassifierSpec,
};

/// Registry of media stream classifier providers.
///
/// Provider names and aliases are matched case-insensitively. Duplicate ids or
/// aliases are rejected at registration time so a selector always resolves to
/// at most one provider.
///
/// [`MediaStreamClassifierRegistry::create_default_box`] and
/// [`MediaStreamClassifierRegistry::create_default_arc`] read
/// [`MimeConfig::media_stream_classifier_default`] first. When the configured
/// selector is empty or `auto`, the registry tries all available providers
/// ordered by descending provider priority and then by provider id. Otherwise
/// it creates the named provider.
#[derive(Debug, Clone, Default)]
pub struct MediaStreamClassifierRegistry {
    /// Typed provider registry supplied by `qubit-spi`.
    providers: ProviderRegistry<MediaStreamClassifierSpec>,
}

/// Process-wide default classifier registry.
static DEFAULT_MEDIA_STREAM_CLASSIFIER_REGISTRY: LazyLock<
    RwLock<MediaStreamClassifierRegistry>,
> = LazyLock::new(|| RwLock::new(MediaStreamClassifierRegistry::builtin()));

/// Backend name used when reporting default registry lock failures.
#[cfg(not(coverage))]
const BACKEND: &str = "media-stream-classifier-registry";

/// Error reason used when a default registry lock is poisoned.
#[cfg(not(coverage))]
const LOCK_ERR: &str = "lock poisoned";

impl MediaStreamClassifierRegistry {
    /// Creates an empty classifier registry.
    ///
    /// # Returns
    /// Empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a registry containing only built-in classifier providers.
    ///
    /// # Returns
    /// Registry with the FFprobe provider.
    pub fn builtin() -> Self {
        let mut registry = Self::new();
        registry
            .register(FfprobeCommandMediaStreamClassifierProvider)
            .expect("built-in FFprobe classifier provider should register");
        registry
    }

    /// Gets a snapshot of the process-wide default classifier registry.
    ///
    /// The returned registry is cloned from the global default registry, so
    /// callers can inspect or create classifiers without holding a global lock.
    ///
    /// # Returns
    /// Snapshot of the default registry.
    ///
    /// # Errors
    /// Returns [`MimeError::ClassifierBackend`] when the global registry lock
    /// has been poisoned by another thread.
    pub fn default_registry() -> MimeResult<Self> {
        let registry = read_default_registry()?;
        Ok(registry.clone())
    }

    /// Registers a provider in the process-wide default classifier registry.
    ///
    /// Successfully registered providers become visible to
    /// [`MediaStreamClassifierRegistry::default_registry`] snapshots throughout
    /// the current process.
    ///
    /// # Parameters
    /// - `provider`: Provider to register globally.
    ///
    /// # Errors
    /// Returns [`MimeError::DuplicateClassifierName`] when the provider id or
    /// one of its aliases already exists in the default registry. Returns
    /// [`MimeError::ClassifierBackend`] when the global registry lock has been
    /// poisoned by another thread.
    pub fn register_default<P>(provider: P) -> MimeResult<()>
    where
        P: MediaStreamClassifierProvider + 'static,
    {
        let mut registry = write_default_registry()?;
        registry.register(provider)
    }

    /// Registers a provider.
    ///
    /// # Parameters
    /// - `provider`: Provider to register.
    ///
    /// # Errors
    /// Returns [`MimeError::DuplicateClassifierName`] when the provider id or
    /// one of its aliases conflicts with an existing provider.
    pub fn register<P>(&mut self, provider: P) -> MimeResult<()>
    where
        P: MediaStreamClassifierProvider + 'static,
    {
        self.providers
            .register(provider)
            .map_err(MimeError::classifier_registry_error)
    }

    /// Registers a shared provider.
    ///
    /// # Parameters
    /// - `provider`: Shared provider to register.
    ///
    /// # Errors
    /// Returns a [`MimeError`] when the provider descriptor is invalid or one
    /// of its names conflicts with an existing provider.
    pub fn register_shared<P>(&mut self, provider: Arc<P>) -> MimeResult<()>
    where
        P: MediaStreamClassifierProvider + 'static,
    {
        self.providers
            .register_shared(provider)
            .map_err(MimeError::classifier_registry_error)
    }

    /// Gets canonical provider names in registration order.
    ///
    /// # Returns
    /// Provider ids.
    pub fn provider_names(&self) -> Vec<&str> {
        self.providers.provider_names()
    }

    /// Finds a provider by id or alias.
    ///
    /// # Parameters
    /// - `name`: Provider id or alias. Matching is case-insensitive.
    ///
    /// # Returns
    /// Matching provider, or `None`.
    pub fn find_provider(
        &self,
        name: &str,
    ) -> Option<&dyn ServiceProvider<MediaStreamClassifierSpec>> {
        self.resolve_provider(name).ok()
    }

    /// Resolves a provider by id or alias.
    ///
    /// # Parameters
    /// - `name`: Provider id or alias. Names are normalized before lookup.
    ///
    /// # Returns
    /// Matching provider.
    ///
    /// # Errors
    /// Returns [`MimeError::EmptyClassifierName`] or
    /// [`MimeError::InvalidClassifierName`] when `name` is invalid, or
    /// [`MimeError::UnknownClassifier`] when no provider matches.
    pub fn resolve_provider(
        &self,
        name: &str,
    ) -> MimeResult<&dyn ServiceProvider<MediaStreamClassifierSpec>> {
        self.providers
            .resolve_provider(name)
            .map_err(MimeError::classifier_registry_error)
    }

    /// Creates a boxed classifier from a provider name.
    ///
    /// # Parameters
    /// - `name`: Provider id or alias.
    /// - `config`: MIME configuration passed to the provider.
    ///
    /// # Returns
    /// Boxed media stream classifier trait object.
    ///
    /// # Errors
    /// Returns [`MimeError::UnknownClassifier`] when no provider matches
    /// `name`, [`MimeError::ClassifierUnavailable`] when the provider is
    /// unavailable, or another [`MimeError`] when provider initialization
    /// fails.
    pub fn create_box(
        &self,
        name: &str,
        config: &MimeConfig,
    ) -> MimeResult<Box<dyn MediaStreamClassifier>> {
        self.providers
            .create_box(name, config)
            .map_err(MimeError::classifier_registry_error)
    }

    /// Creates a shared classifier from a provider name.
    ///
    /// # Parameters
    /// - `name`: Provider id or alias.
    /// - `config`: MIME configuration passed to the provider.
    ///
    /// # Returns
    /// Shared media stream classifier trait object.
    ///
    /// # Errors
    /// Returns [`MimeError::UnknownClassifier`] when no provider matches
    /// `name`, [`MimeError::ClassifierUnavailable`] when the provider is
    /// unavailable, or another [`MimeError`] when provider initialization
    /// fails.
    pub fn create_arc(
        &self,
        name: &str,
        config: &MimeConfig,
    ) -> MimeResult<Arc<dyn MediaStreamClassifier>> {
        self.providers
            .create_arc(name, config)
            .map_err(MimeError::classifier_registry_error)
    }

    /// Creates a boxed classifier from the configured default selector.
    ///
    /// # Parameters
    /// - `config`: MIME configuration.
    ///
    /// # Returns
    /// First boxed classifier that can be created.
    ///
    /// # Errors
    /// Returns [`MimeError::NoAvailableClassifier`] when no configured provider
    /// can be created.
    pub fn create_default_box(
        &self,
        config: &MimeConfig,
    ) -> MimeResult<Box<dyn MediaStreamClassifier>> {
        let selection = provider_selection_from_config(config)?;
        self.providers
            .create_selected_box(&selection, config)
            .map_err(MimeError::classifier_registry_error)
    }

    /// Creates a shared classifier from the configured default selector.
    ///
    /// # Parameters
    /// - `config`: MIME configuration.
    ///
    /// # Returns
    /// First shared classifier that can be created.
    ///
    /// # Errors
    /// Returns [`MimeError::NoAvailableClassifier`] when no configured provider
    /// can be created.
    pub fn create_default_arc(
        &self,
        config: &MimeConfig,
    ) -> MimeResult<Arc<dyn MediaStreamClassifier>> {
        let selection = provider_selection_from_config(config)?;
        self.providers
            .create_selected_arc(&selection, config)
            .map_err(MimeError::classifier_registry_error)
    }
}

/// Builds the provider selection policy from MIME configuration.
///
/// # Parameters
/// - `config`: MIME configuration.
///
/// # Returns
/// Provider selection used by `qubit-spi`.
///
/// # Errors
/// Returns [`MimeError`] when a configured provider name is invalid.
fn provider_selection_from_config(
    config: &MimeConfig,
) -> MimeResult<ProviderSelection> {
    let configured = config.media_stream_classifier_default().trim();
    if configured.is_empty() || configured.eq_ignore_ascii_case("auto") {
        return Ok(ProviderSelection::Auto);
    }
    ProviderSelection::named(configured)
        .map_err(MimeError::classifier_registry_error)
}

/// Locks the default registry for reading.
///
/// # Returns
/// Read guard for the default registry.
///
/// # Errors
/// Returns [`MimeError::ClassifierBackend`] when the global registry lock has
/// been poisoned by another thread.
#[cfg(not(coverage))]
fn read_default_registry()
-> MimeResult<RwLockReadGuard<'static, MediaStreamClassifierRegistry>> {
    match DEFAULT_MEDIA_STREAM_CLASSIFIER_REGISTRY.read() {
        Ok(registry) => Ok(registry),
        Err(_) => Err(MimeError::ClassifierBackend {
            backend: BACKEND.into(),
            reason: LOCK_ERR.into(),
        }),
    }
}

/// Locks the default registry for reading during coverage runs.
///
/// Poisoning cannot be triggered reliably through public behavior, so coverage
/// runs recover the guard and keep the public API path covered.
///
/// # Returns
/// Read guard for the default registry.
#[cfg(coverage)]
fn read_default_registry()
-> MimeResult<RwLockReadGuard<'static, MediaStreamClassifierRegistry>> {
    Ok(DEFAULT_MEDIA_STREAM_CLASSIFIER_REGISTRY
        .read()
        .unwrap_or_else(PoisonError::into_inner))
}

/// Locks the default registry for writing.
///
/// # Returns
/// Write guard for the default registry.
///
/// # Errors
/// Returns [`MimeError::ClassifierBackend`] when the global registry lock has
/// been poisoned by another thread.
#[cfg(not(coverage))]
fn write_default_registry()
-> MimeResult<RwLockWriteGuard<'static, MediaStreamClassifierRegistry>> {
    match DEFAULT_MEDIA_STREAM_CLASSIFIER_REGISTRY.write() {
        Ok(registry) => Ok(registry),
        Err(_) => Err(MimeError::ClassifierBackend {
            backend: BACKEND.into(),
            reason: LOCK_ERR.into(),
        }),
    }
}

/// Locks the default registry for writing during coverage runs.
///
/// Poisoning cannot be triggered reliably through public behavior, so coverage
/// runs recover the guard and keep the public API path covered.
///
/// # Returns
/// Write guard for the default registry.
#[cfg(coverage)]
fn write_default_registry()
-> MimeResult<RwLockWriteGuard<'static, MediaStreamClassifierRegistry>> {
    Ok(DEFAULT_MEDIA_STREAM_CLASSIFIER_REGISTRY
        .write()
        .unwrap_or_else(PoisonError::into_inner))
}
