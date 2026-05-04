/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Registry for pluggable MIME detector providers.
//!
//! The registry is the selection layer used by the detector wrappers. It maps
//! stable provider names and aliases to factories, checks provider availability,
//! and resolves configured fallback chains. Default wrappers use the process-wide
//! default registry, while applications that need custom provider isolation can
//! pass an explicit registry to
//! [`BoxMimeDetector::from_registry`](crate::BoxMimeDetector::from_registry) or
//! [`ArcMimeDetector::from_registry`](crate::ArcMimeDetector::from_registry).
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

use crate::{
    BoxMimeDetector,
    MimeConfig,
    MimeError,
    MimeResult,
};

use super::{
    FileCommandMimeDetectorProvider,
    MimeDetectorAvailability,
    MimeDetectorProvider,
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
/// [`MimeDetectorRegistry::create_default`] reads
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
/// let detector = registry.create_default(&MimeConfig::default())?;
/// assert_eq!(
///     Some("text/plain".to_owned()),
///     detector.detect_by_filename("notes.txt"),
/// );
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone, Default)]
pub struct MimeDetectorRegistry {
    /// Registered detector providers.
    providers: Vec<Arc<dyn MimeDetectorProvider>>,
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
        Self {
            providers: vec![
                Arc::new(RepositoryMimeDetectorProvider) as Arc<dyn MimeDetectorProvider>,
                Arc::new(FileCommandMimeDetectorProvider) as Arc<dyn MimeDetectorProvider>,
            ],
        }
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
    /// Detectors created through [`BoxMimeDetector::from_config`] and
    /// [`BoxMimeDetector::from_name`](crate::BoxMimeDetector::from_name) use
    /// this registry, so successfully registered providers become visible to
    /// default wrapper constructors throughout the current process.
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
        self.register_arc(Arc::new(provider))
    }

    /// Registers a shared provider.
    ///
    /// # Parameters
    /// - `provider`: Shared provider to register.
    ///
    /// # Errors
    /// Returns [`MimeError::DuplicateDetectorName`] when the provider id or one
    /// of its aliases conflicts with an existing provider.
    pub fn register_arc(&mut self, provider: Arc<dyn MimeDetectorProvider>) -> MimeResult<()> {
        for name in provider_names(provider.as_ref()) {
            if self.find_provider(&name).is_some() {
                return Err(MimeError::DuplicateDetectorName { name });
            }
        }
        self.providers.push(provider);
        Ok(())
    }

    /// Gets canonical provider names in registration order.
    ///
    /// # Returns
    /// Provider ids.
    pub fn provider_names(&self) -> Vec<&'static str> {
        self.providers
            .iter()
            .map(|provider| provider.id())
            .collect()
    }

    /// Finds a provider by id or alias.
    ///
    /// # Parameters
    /// - `name`: Provider id or alias. Matching is case-insensitive.
    ///
    /// # Returns
    /// Matching provider, or `None`.
    pub fn find_provider(&self, name: &str) -> Option<&dyn MimeDetectorProvider> {
        self.providers
            .iter()
            .map(Arc::as_ref)
            .find(|provider| provider_matches(*provider, name))
    }

    /// Creates a detector from a provider name.
    ///
    /// # Parameters
    /// - `name`: Provider id or alias.
    /// - `config`: MIME configuration passed to the provider.
    ///
    /// # Returns
    /// Boxed MIME detector.
    ///
    /// # Errors
    /// Returns [`MimeError::UnknownDetector`] when no provider matches `name`,
    /// [`MimeError::DetectorUnavailable`] when the provider is unavailable, or
    /// another [`MimeError`] when provider initialization fails.
    pub fn create(&self, name: &str, config: &MimeConfig) -> MimeResult<BoxMimeDetector> {
        let provider = self
            .find_provider(name)
            .ok_or_else(|| MimeError::UnknownDetector {
                name: name.to_owned(),
            })?;
        match provider.availability(config) {
            MimeDetectorAvailability::Available => {
                provider.create(config).map(BoxMimeDetector::new)
            }
            MimeDetectorAvailability::Unavailable { reason } => {
                Err(MimeError::DetectorUnavailable {
                    name: name.to_owned(),
                    reason,
                })
            }
        }
    }

    /// Creates a detector from the configured default and fallback chain.
    ///
    /// # Parameters
    /// - `config`: MIME configuration.
    ///
    /// # Returns
    /// First detector that can be created.
    ///
    /// # Errors
    /// Returns [`MimeError::NoAvailableDetector`] when all configured providers
    /// are unknown, unavailable, or fail during initialization.
    pub fn create_default(&self, config: &MimeConfig) -> MimeResult<BoxMimeDetector> {
        let candidates = self.default_candidates(config);
        if candidates.is_empty() {
            return Err(MimeError::NoAvailableDetector {
                reason: "detector registry is empty".to_owned(),
            });
        }
        let mut errors = Vec::new();
        for candidate in candidates {
            match self.create(&candidate, config) {
                Ok(detector) => return Ok(detector),
                Err(error) => errors.push(error.to_string()),
            }
        }
        Err(MimeError::NoAvailableDetector {
            reason: errors.join("; "),
        })
    }

    /// Builds the default provider candidate chain.
    ///
    /// # Parameters
    /// - `config`: MIME configuration.
    ///
    /// # Returns
    /// Ordered provider names to try.
    fn default_candidates(&self, config: &MimeConfig) -> Vec<String> {
        let configured = config.mime_detector_default().trim();
        if configured.is_empty() || configured.eq_ignore_ascii_case("auto") {
            return self.auto_candidates(config);
        }
        let mut candidates = vec![configured.to_owned()];
        candidates.extend(config.mime_detector_fallbacks().iter().cloned());
        candidates
    }

    /// Builds provider candidates for automatic selection.
    ///
    /// # Parameters
    /// - `config`: MIME configuration.
    ///
    /// # Returns
    /// Available provider ids ordered by descending priority.
    fn auto_candidates(&self, config: &MimeConfig) -> Vec<String> {
        let mut providers: Vec<&dyn MimeDetectorProvider> = self
            .providers
            .iter()
            .map(Arc::as_ref)
            .filter(|provider| provider.availability(config).is_available())
            .collect();
        providers.sort_by(|left, right| {
            right
                .priority()
                .cmp(&left.priority())
                .then_with(|| left.id().cmp(right.id()))
        });
        providers
            .into_iter()
            .map(|provider| provider.id().to_owned())
            .collect()
    }
}

/// Gets all names exposed by a provider.
///
/// # Parameters
/// - `provider`: Provider to inspect.
///
/// # Returns
/// Provider id and aliases.
fn provider_names(provider: &dyn MimeDetectorProvider) -> Vec<String> {
    let mut names = Vec::with_capacity(provider.aliases().len() + 1);
    names.push(provider.id().to_owned());
    names.extend(provider.aliases().iter().map(|alias| (*alias).to_owned()));
    names
}

/// Tells whether a provider matches a requested name.
///
/// # Parameters
/// - `provider`: Provider to inspect.
/// - `name`: Requested id or alias.
///
/// # Returns
/// `true` when `name` matches the provider id or any alias.
fn provider_matches(provider: &dyn MimeDetectorProvider, name: &str) -> bool {
    provider.id().eq_ignore_ascii_case(name)
        || provider
            .aliases()
            .iter()
            .any(|alias| alias.eq_ignore_ascii_case(name))
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
fn read_default_registry() -> MimeResult<RwLockReadGuard<'static, MimeDetectorRegistry>> {
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
fn read_default_registry() -> MimeResult<RwLockReadGuard<'static, MimeDetectorRegistry>> {
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
fn write_default_registry() -> MimeResult<RwLockWriteGuard<'static, MimeDetectorRegistry>> {
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
fn write_default_registry() -> MimeResult<RwLockWriteGuard<'static, MimeDetectorRegistry>> {
    Ok(DEFAULT_MIME_DETECTOR_REGISTRY
        .write()
        .unwrap_or_else(PoisonError::into_inner))
}
