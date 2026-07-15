// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Provider for the built-in system `file` command MIME detector.

use std::sync::Arc;

use crate::{
    FileCommandMimeDetector,
    MimeConfig,
    MimeDetector,
    ProviderDescriptor,
    ProviderError,
    ProviderId,
    ServiceProvider,
};

use super::MimeDetectorSpec;

/// Provider for the built-in system `file` command detector.
#[derive(Debug, Clone, Copy, Default)]
pub struct FileCommandMimeDetectorProvider;

impl ServiceProvider<MimeDetectorSpec> for FileCommandMimeDetectorProvider {
    fn create(
        &self,
        config: &MimeConfig,
    ) -> Result<Arc<dyn MimeDetector>, ProviderError> {
        if !FileCommandMimeDetector::is_available() {
            return Err(ProviderError::unavailable(
                "`file` command is not available",
            ));
        }
        Ok(Arc::new(FileCommandMimeDetector::from_mime_config(
            config.clone(),
        )))
    }
}

/// Gets the immutable descriptor for the `file` command detector provider.
#[must_use]
pub fn file_command_mime_detector_descriptor() -> ProviderDescriptor {
    ProviderDescriptor::new(
        ProviderId::new("file").expect("built-in provider ID should be valid"),
    )
    .with_aliases(["file-command", "file-command-mime-detector"])
    .expect("built-in file detector aliases should be valid")
    .with_priority(10)
}
