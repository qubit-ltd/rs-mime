// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Provider for the built-in system `file` command MIME detector.

use crate::{
    FileCommandMimeDetector,
    MimeConfig,
    MimeDetector,
    ProviderAvailability,
    ProviderCreateError,
    ProviderDescriptor,
    ProviderRegistryError,
    ServiceProvider,
};

use super::{
    MimeDetectorAvailability,
    MimeDetectorSpec,
};

/// Provider for the built-in system `file` command detector.
#[derive(Debug, Clone, Copy, Default)]
pub struct FileCommandMimeDetectorProvider;

impl ServiceProvider<MimeDetectorSpec> for FileCommandMimeDetectorProvider {
    /// Gets file command detector metadata.
    fn descriptor(&self) -> Result<ProviderDescriptor, ProviderRegistryError> {
        let descriptor = ProviderDescriptor::new("file")
            .expect("built-in file detector provider id should be valid")
            .with_aliases(&["file-command", "file-command-mime-detector"])
            .expect("built-in file detector aliases should be valid")
            .with_priority(10);
        Ok(descriptor)
    }

    /// Checks whether the `file` command is available.
    fn availability(&self, _config: &MimeConfig) -> MimeDetectorAvailability {
        if FileCommandMimeDetector::is_available() {
            ProviderAvailability::Available
        } else {
            ProviderAvailability::unavailable("`file` command is not available")
        }
    }

    /// Creates a file-command-backed detector.
    fn create_box(
        &self,
        config: &MimeConfig,
    ) -> Result<Box<dyn MimeDetector>, ProviderCreateError> {
        Ok(Box::new(FileCommandMimeDetector::from_mime_config(
            config.clone(),
        )))
    }
}
