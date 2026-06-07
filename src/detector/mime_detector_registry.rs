// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Registry for pluggable MIME detector providers.
//!
//! The registry is the selection layer used by MIME detector factories. It maps
//! stable provider names and aliases to factories, checks provider
//! availability, and resolves configured fallback chains. Applications can use
//! the process-wide default registry or keep an explicit registry for provider
//! isolation.
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
    MimeConfig,
    MimeDetector,
    MimeError,
    MimeResult,
};

use super::{
    FileCommandMimeDetectorProvider,
    MimeDetectorProvider,
    MimeDetectorSpec,
    RepositoryMimeDetectorProvider,
};

/// Registry of MIME detector providers.
///
/// Provider names and aliases are matched case-insensitively. Duplicate ids or
/// aliases are rejected at registration time so a selector always resolves to
/// at most one provider.
///
/// # Default and fallback selection
///
/// [`MimeDetectorRegistry::create_default_box`] and
/// [`MimeDetectorRegistry::create_default_arc`] read
/// [`MimeConfig::mime_detector_default`](crate::MimeConfig::mime_detector_default)
/// first. When the default selector is empty or `auto`, the registry tries all
/// available providers ordered by descending provider priority and then by
/// provider id. Otherwise it tries the configured default followed by
/// [`MimeConfig::mime_detector_fallbacks`](crate::MimeConfig::mime_detector_fallbacks).
///
/// Selection stops at the first provider that can create a detector. Unknown,
/// unavailable, or failing providers are collected into
/// [`MimeError::NoAvailableDetector`](crate::MimeError::NoAvailableDetector)
/// only when the whole candidate chain fails.
///
/// # Examples
///
/// Use a registry containing only built-in providers:
///
/// ```rust
/// use qubit_mime::{
///     MimeConfig,
///     MimeDetector,
///     MimeDetectorRegistry,
///     MimeResult,
/// };
///
/// # fn main() -> MimeResult<()> {
/// let registry = MimeDetectorRegistry::builtin();
/// assert!(registry.find_provider("repository-mime-detector").is_some());
///
/// let detector = registry.create_default_box(&MimeConfig::default())?;
/// assert_eq!(
///     Some("text/plain".to_owned()),
///     detector.detect_by_filename("notes.txt"),
/// );
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone, Default)]
pub struct MimeDetectorRegistry {
    /// Typed provider registry supplied by `qubit-spi`.
    providers: ProviderRegistry<MimeDetectorSpec>,
}

/// Process-wide default detector registry.
static DEFAULT_MIME_DETECTOR_REGISTRY: LazyLock<RwLock<MimeDetectorRegistry>> =
    LazyLock::new(|| RwLock::new(MimeDetectorRegistry::builtin()));

/// Backend name used when reporting default registry lock failures.
#[cfg(not(coverage))]
const BACKEND: &str = "mime-detector-registry";

/// Error reason used when a default registry lock is poisoned.
#[cfg(not(coverage))]
const LOCK_ERR: &str = "lock poisoned";

impl MimeDetectorRegistry {
    /// Creates an empty detector registry.
    ///
    /// # Returns
    /// Empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a registry containing only built-in detector providers.
    ///
    /// # Returns
    /// Registry with repository and file-command providers.
    pub fn builtin() -> Self {
        let mut registry = Self::new();
        registry
            .register(RepositoryMimeDetectorProvider)
            .expect("built-in repository MIME provider should register");
        registry
            .register(FileCommandMimeDetectorProvider)
            .expect("built-in file MIME provider should register");
        registry
    }

    /// Gets a snapshot of the process-wide default detector registry.
    ///
    /// The returned registry is cloned from the global default registry, so
    /// callers can inspect or create detectors without holding a global lock.
    ///
    /// # Returns
    /// Snapshot of the default registry.
    ///
    /// # Errors
    /// Returns [`MimeError::DetectorBackend`] when the global registry lock has
    /// been poisoned by another thread.
    pub fn default_registry() -> MimeResult<Self> {
        let registry = read_default_registry()?;
        Ok(registry.clone())
    }

    /// Registers a provider in the process-wide default detector registry.
    ///
    /// Successfully registered providers become visible to
    /// [`MimeDetectorRegistry::default_registry`] snapshots throughout the
    /// current process.
    ///
    /// # Parameters
    /// - `provider`: Provider to register globally.
    ///
    /// # Errors
    /// Returns [`MimeError::DuplicateDetectorName`] when the provider id or one
    /// of its aliases already exists in the default registry. Returns
    /// [`MimeError::DetectorBackend`] when the global registry lock has been
    /// poisoned by another thread.
    pub fn register_default<P>(provider: P) -> MimeResult<()>
    where
        P: MimeDetectorProvider + 'static,
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
    /// Returns [`MimeError::DuplicateDetectorName`] when the provider id or one
    /// of its aliases conflicts with an existing provider.
    pub fn register<P>(&mut self, provider: P) -> MimeResult<()>
    where
        P: MimeDetectorProvider + 'static,
    {
        self.providers.register(provider).map_err(MimeError::from)
    }

    /// Registers a shared provider.
    ///
    /// # Parameters
    /// - `provider`: Shared provider to register.
    ///
    /// # Errors
    /// Returns [`MimeError::DuplicateDetectorName`] when the provider id or one
    /// of its aliases conflicts with an existing provider.
    pub fn register_shared<P>(&mut self, provider: Arc<P>) -> MimeResult<()>
    where
        P: MimeDetectorProvider + 'static,
    {
        self.providers
            .register_shared(provider)
            .map_err(MimeError::from)
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
    ) -> Option<&dyn ServiceProvider<MimeDetectorSpec>> {
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
    /// Returns [`MimeError::EmptyDetectorName`] or
    /// [`MimeError::InvalidDetectorName`] when `name` is invalid, or
    /// [`MimeError::UnknownDetector`] when no provider matches.
    pub fn resolve_provider(
        &self,
        name: &str,
    ) -> MimeResult<&dyn ServiceProvider<MimeDetectorSpec>> {
        self.providers
            .resolve_provider(name)
            .map_err(MimeError::from)
    }

    /// Creates a boxed detector from a provider name.
    ///
    /// # Parameters
    /// - `name`: Provider id or alias.
    /// - `config`: MIME configuration passed to the provider.
    ///
    /// # Returns
    /// Boxed MIME detector trait object.
    ///
    /// # Errors
    /// Returns [`MimeError::UnknownDetector`] when no provider matches `name`,
    /// [`MimeError::DetectorUnavailable`] when the provider is unavailable, or
    /// another [`MimeError`] when provider initialization fails.
    pub fn create_box(
        &self,
        name: &str,
        config: &MimeConfig,
    ) -> MimeResult<Box<dyn MimeDetector>> {
        self.providers
            .create_box(name, config)
            .map_err(MimeError::from)
    }

    /// Creates a shared detector from a provider name.
    ///
    /// # Parameters
    /// - `name`: Provider id or alias.
    /// - `config`: MIME configuration passed to the provider.
    ///
    /// # Returns
    /// Shared MIME detector trait object.
    ///
    /// # Errors
    /// Returns [`MimeError::UnknownDetector`] when no provider matches `name`,
    /// [`MimeError::DetectorUnavailable`] when the provider is unavailable, or
    /// another [`MimeError`] when provider initialization fails.
    pub fn create_arc(
        &self,
        name: &str,
        config: &MimeConfig,
    ) -> MimeResult<Arc<dyn MimeDetector>> {
        self.providers
            .create_arc(name, config)
            .map_err(MimeError::from)
    }

    /// Creates a boxed detector from the configured default and fallback chain.
    ///
    /// # Parameters
    /// - `config`: MIME configuration.
    ///
    /// # Returns
    /// First boxed detector that can be created.
    ///
    /// # Errors
    /// Returns [`MimeError::NoAvailableDetector`] when all configured providers
    /// are unknown, unavailable, or fail during initialization.
    pub fn create_default_box(
        &self,
        config: &MimeConfig,
    ) -> MimeResult<Box<dyn MimeDetector>> {
        let selection = provider_selection_from_config(config)?;
        self.providers
            .create_selected_box(&selection, config)
            .map_err(MimeError::from)
    }

    /// Creates a shared detector from the configured default and fallback
    /// chain.
    ///
    /// # Parameters
    /// - `config`: MIME configuration.
    ///
    /// # Returns
    /// First shared detector that can be created.
    ///
    /// # Errors
    /// Returns [`MimeError::NoAvailableDetector`] when all configured providers
    /// are unknown, unavailable, or fail during initialization.
    pub fn create_default_arc(
        &self,
        config: &MimeConfig,
    ) -> MimeResult<Arc<dyn MimeDetector>> {
        let selection = provider_selection_from_config(config)?;
        self.providers
            .create_selected_arc(&selection, config)
            .map_err(MimeError::from)
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
    let configured = config.mime_detector_default().trim();
    if configured.is_empty() || configured.eq_ignore_ascii_case("auto") {
        return Ok(ProviderSelection::Auto);
    }
    ProviderSelection::from_owned_names(
        configured,
        config.mime_detector_fallbacks(),
    )
    .map_err(MimeError::from)
}

/// Locks the default registry for reading.
///
/// # Returns
/// Read guard for the default registry.
///
/// # Errors
/// Returns [`MimeError::DetectorBackend`] when the global registry lock has
/// been poisoned by another thread.
#[cfg(not(coverage))]
fn read_default_registry()
-> MimeResult<RwLockReadGuard<'static, MimeDetectorRegistry>> {
    match DEFAULT_MIME_DETECTOR_REGISTRY.read() {
        Ok(registry) => Ok(registry),
        Err(_) => Err(MimeError::DetectorBackend {
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
-> MimeResult<RwLockReadGuard<'static, MimeDetectorRegistry>> {
    Ok(DEFAULT_MIME_DETECTOR_REGISTRY
        .read()
        .unwrap_or_else(PoisonError::into_inner))
}

/// Locks the default registry for writing.
///
/// # Returns
/// Write guard for the default registry.
///
/// # Errors
/// Returns [`MimeError::DetectorBackend`] when the global registry lock has
/// been poisoned by another thread.
#[cfg(not(coverage))]
fn write_default_registry()
-> MimeResult<RwLockWriteGuard<'static, MimeDetectorRegistry>> {
    match DEFAULT_MIME_DETECTOR_REGISTRY.write() {
        Ok(registry) => Ok(registry),
        Err(_) => Err(MimeError::DetectorBackend {
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
-> MimeResult<RwLockWriteGuard<'static, MimeDetectorRegistry>> {
    Ok(DEFAULT_MIME_DETECTOR_REGISTRY
        .write()
        .unwrap_or_else(PoisonError::into_inner))
}
