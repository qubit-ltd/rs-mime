// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Provider contract for pluggable MIME detector implementations.
//!
//! The MIME detector SPI is a domain binding over [`qubit_spi`]. Providers
//! implement [`qubit_spi::ProviderDefinition<MimeDetectorSpec>`], which
//! combines detector creation with self-described registration metadata.

use qubit_spi::ProviderDefinition;

use super::MimeDetectorSpec;

/// Marker trait for MIME detector providers.
///
/// Implement [`ProviderDefinition<MimeDetectorSpec>`] for the concrete
/// provider type. This marker keeps public registry bounds MIME-specific while
/// ensuring every registrable detector supplies its own descriptor.
///
/// # Examples
///
/// Use a built-in provider to satisfy a MIME-specific generic bound:
///
/// ```
/// use qubit_mime::MimeDetectorProvider;
/// use qubit_mime::RepositoryMimeDetectorProvider;
///
/// fn require_mime_detector_provider<P: MimeDetectorProvider>() {}
///
/// require_mime_detector_provider::<RepositoryMimeDetectorProvider>();
/// ```
pub trait MimeDetectorProvider: ProviderDefinition<MimeDetectorSpec> {
    // empty
}

impl<T> MimeDetectorProvider for T where T: ProviderDefinition<MimeDetectorSpec> + ?Sized {}

/// Explicit async capability offered by a detector provider.
pub trait AsyncMimeDetectorProvider: qubit_spi::ProviderMetadata + std::fmt::Debug + Send + Sync {
    /// Creates an async backend bound to this operation service set.
    fn create_async_backend(
        &self,
        config: &crate::MimeConfig,
        services: &crate::MimeExecutionServices,
    ) -> Result<std::sync::Arc<dyn crate::AsyncMimeDetectorBackend>, crate::MimeError>;

    /// Creates an async detector with an optional async media-stream
    /// classifier attached to its selection core.
    fn create_async_backend_with_classifier(
        &self,
        config: &crate::MimeConfig,
        services: &crate::MimeExecutionServices,
        classifier: Option<std::sync::Arc<dyn crate::AsyncMediaStreamClassifierBackend>>,
    ) -> Result<std::sync::Arc<dyn crate::AsyncMimeDetectorBackend>, crate::MimeError> {
        if classifier.is_some() {
            return Err(crate::MimeError::UnsupportedCapability {
                capability: crate::MimeCapability::SameObjectRefinement,
            });
        }
        self.create_async_backend(config, services)
    }
}
