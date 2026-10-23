// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Provider for the built-in repository-backed MIME detector.

use std::sync::Arc;

use qubit_spi::ProviderDescriptor;
use qubit_spi::ProviderMetadata;
use qubit_spi::ServiceProvider;
use qubit_spi::error::ProviderFailure;
use qubit_spi::provider_descriptor;

use super::MimeDetectorSpec;
use crate::MimeConfig;
use crate::MimeDetector;
use crate::MimeError;
use crate::RepositoryMimeDetector;

/// Creates detectors backed by the built-in MIME repository.
///
/// Register this provider when constructing a custom MIME detector registry.
/// The built-in registry already includes it.
///
/// # Examples
///
/// ```
/// use qubit_mime::MimeDetectorRegistry;
/// use qubit_mime::RepositoryMimeDetectorProvider;
///
/// let registry = MimeDetectorRegistry::default();
/// registry
///     .register(RepositoryMimeDetectorProvider)
///     .expect("the empty registry should accept the provider");
/// assert_eq!("repository", registry.provider_ids()[0].as_str());
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct RepositoryMimeDetectorProvider;

impl ServiceProvider<MimeDetectorSpec> for RepositoryMimeDetectorProvider {
    /// Creates a repository-backed detector from the supplied configuration.
    ///
    /// # Parameters
    ///
    /// * `config` - Configuration cloned into the new detector.
    ///
    /// # Returns
    ///
    /// A shared detector using the built-in MIME repository.
    ///
    /// # Errors
    ///
    /// This provider does not currently return an error.
    #[inline]
    fn create_configured(&self, config: &MimeConfig) -> Result<Arc<dyn MimeDetector>, ProviderFailure<MimeError>> {
        Ok(Arc::new(RepositoryMimeDetector::from_mime_config(config.clone())))
    }
}

impl ProviderMetadata for RepositoryMimeDetectorProvider {
    /// Returns the stable repository provider identity.
    ///
    /// # Returns
    ///
    /// The `repository` provider descriptor and its accepted alias.
    fn descriptor(&self) -> ProviderDescriptor {
        provider_descriptor!("repository", aliases: ["repository-mime-detector"])
    }
}
