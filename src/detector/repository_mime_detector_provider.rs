/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Provider for the built-in repository-backed MIME detector.

use crate::{
    MimeConfig,
    MimeDetector,
    ProviderCreateError,
    ProviderDescriptor,
    ProviderRegistryError,
    RepositoryMimeDetector,
    ServiceProvider,
};

use super::MimeDetectorSpec;

/// Provider for the built-in repository-backed detector.
#[derive(Debug, Clone, Copy, Default)]
pub struct RepositoryMimeDetectorProvider;

impl ServiceProvider<MimeDetectorSpec> for RepositoryMimeDetectorProvider {
    /// Gets repository detector metadata.
    fn descriptor(&self) -> Result<ProviderDescriptor, ProviderRegistryError> {
        let descriptor = ProviderDescriptor::new("repository")
            .expect("built-in repository detector provider id should be valid")
            .with_aliases(&["repository-mime-detector"])
            .expect("built-in repository detector aliases should be valid");
        Ok(descriptor)
    }

    /// Creates a repository-backed detector.
    fn create_box(&self, config: &MimeConfig) -> Result<Box<dyn MimeDetector>, ProviderCreateError> {
        Ok(Box::new(RepositoryMimeDetector::from_mime_config(config.clone())))
    }
}
