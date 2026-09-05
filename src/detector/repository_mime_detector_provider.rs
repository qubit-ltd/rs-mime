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

/// Provider for the built-in repository-backed detector.
#[derive(Debug, Clone, Copy, Default)]
pub struct RepositoryMimeDetectorProvider;

impl ServiceProvider<MimeDetectorSpec> for RepositoryMimeDetectorProvider {
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
