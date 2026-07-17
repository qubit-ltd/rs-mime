// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Provider for the built-in repository-backed MIME detector.

use std::sync::Arc;

use qubit_spi::error::ProviderCreationError;
use qubit_spi::{
    ProviderDefinition,
    ProviderDescriptor,
    ProviderId,
    ServiceProvider,
};

use crate::{
    MimeConfig,
    MimeDetector,
    RepositoryMimeDetector,
};

use super::MimeDetectorSpec;

/// Provider for the built-in repository-backed detector.
#[derive(Debug, Clone, Copy, Default)]
pub struct RepositoryMimeDetectorProvider;

impl ServiceProvider<MimeDetectorSpec> for RepositoryMimeDetectorProvider {
    #[inline]
    fn create(
        &self,
        config: &MimeConfig,
    ) -> Result<Arc<dyn MimeDetector>, ProviderCreationError> {
        Ok(Arc::new(RepositoryMimeDetector::from_mime_config(
            config.clone(),
        )))
    }
}

impl ProviderDefinition<MimeDetectorSpec> for RepositoryMimeDetectorProvider {
    /// Returns the stable repository provider identity.
    ///
    /// # Returns
    ///
    /// The `repository` provider descriptor and its accepted alias.
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor::new(
            ProviderId::new("repository")
                .expect("built-in provider ID should be valid"),
        )
        .with_aliases(["repository-mime-detector"])
        .expect("built-in repository detector aliases should be valid")
    }
}
