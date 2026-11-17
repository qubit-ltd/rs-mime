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
use crate::AsyncMimeDetectorBackend;
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

impl RepositoryMimeDetectorProvider {
    /// Creates the repository backend for callers explicitly selecting the
    /// asynchronous MIME interface. Construction uses the already-loaded
    /// bundled repository and performs no I/O or XML parsing.
    ///
    /// # Parameters
    /// - `config`: MIME settings copied into the detector backend.
    ///
    /// # Returns
    /// An async backend using the bundled MIME repository.
    #[must_use]
    pub fn create_async_backend(&self, config: &MimeConfig) -> Arc<dyn AsyncMimeDetectorBackend> {
        Arc::new(RepositoryMimeDetector::from_mime_config(config.clone()))
    }
}

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

impl super::AsyncMimeDetectorProvider for RepositoryMimeDetectorProvider {
    /// Creates the repository backend for managed operations.
    ///
    /// # Parameters
    /// - `config`: MIME settings copied into the backend.
    /// - `_services`: Shared execution services; unused by this provider.
    ///
    /// # Returns
    /// The repository-backed async detector.
    ///
    /// # Errors
    /// This built-in provider does not currently return an error.
    fn create_async_backend(
        &self,
        config: &MimeConfig,
        _services: &crate::MimeExecutionServices,
    ) -> Result<Arc<dyn AsyncMimeDetectorBackend>, MimeError> {
        Ok(RepositoryMimeDetectorProvider::create_async_backend(self, config))
    }

    /// Creates the backend and attaches an optional stream classifier.
    ///
    /// # Parameters
    /// - `config`: MIME settings copied into the backend.
    /// - `_services`: Shared execution services; unused by this provider.
    /// - `classifier`: Optional classifier used for async refinement.
    ///
    /// # Returns
    /// The configured repository-backed async detector.
    ///
    /// # Errors
    /// This built-in provider does not currently return an error.
    fn create_async_backend_with_classifier(
        &self,
        config: &MimeConfig,
        _services: &crate::MimeExecutionServices,
        classifier: Option<Arc<dyn crate::AsyncMediaStreamClassifierBackend>>,
    ) -> Result<Arc<dyn AsyncMimeDetectorBackend>, MimeError> {
        let detector =
            RepositoryMimeDetector::from_mime_config(config.clone()).with_async_media_stream_classifier(classifier);
        Ok(Arc::new(detector))
    }
}

#[cfg(test)]
mod tests {
    use super::RepositoryMimeDetectorProvider;
    use crate::AsyncMimeDetectorProvider;
    use crate::ContentRequirement;
    use crate::FixedMimeExecutionPort;
    use crate::MimeConfig;
    use crate::MimeExecutionServices;
    use crate::RandomAccessCapability;

    fn services() -> MimeExecutionServices {
        let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
        let group = manager.group(qubit_operation::GroupOptions { capacity: 1 }).unwrap();
        MimeExecutionServices::new(
            manager,
            group,
            std::sync::Arc::new(FixedMimeExecutionPort::new(1, 1)),
            16,
        )
        .unwrap()
    }

    #[test]
    fn test_repository_provider_constructors_return_prefix_async_backends() {
        let provider = RepositoryMimeDetectorProvider;
        let config = MimeConfig::default();
        let direct = provider.create_async_backend(&config);
        assert!(matches!(direct.content_requirement(), ContentRequirement::Prefix(bytes) if bytes > 0));
        assert_eq!(RandomAccessCapability::Unsupported, direct.random_access_capability());

        let services = services();
        let trait_backend = <RepositoryMimeDetectorProvider as AsyncMimeDetectorProvider>::create_async_backend(
            &provider, &config, &services,
        )
        .expect("trait factory should use the bundled repository without external work");
        assert_eq!(direct.content_requirement(), trait_backend.content_requirement());
        assert_eq!(
            direct.random_access_capability(),
            trait_backend.random_access_capability()
        );
    }
}
