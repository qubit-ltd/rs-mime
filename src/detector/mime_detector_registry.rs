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

use std::sync::Arc;

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
#[derive(Debug, Clone, Default)]
pub struct MimeDetectorRegistry {
    /// Registered detector providers.
    providers: Vec<Arc<dyn MimeDetectorProvider>>,
}

impl MimeDetectorRegistry {
    /// Creates an empty detector registry.
    ///
    /// # Returns
    /// Empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a registry containing built-in detector providers.
    ///
    /// # Returns
    /// Registry with repository and file-command providers.
    pub fn with_builtin() -> Self {
        let mut registry = Self::new();
        registry
            .register(RepositoryMimeDetectorProvider)
            .expect("built-in repository provider should register");
        registry
            .register(FileCommandMimeDetectorProvider)
            .expect("built-in file command provider should register");
        registry
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
