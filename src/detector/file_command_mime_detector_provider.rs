// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Provider for the built-in system `file` command MIME detector.

use std::sync::Arc;

use qubit_spi::error::{
    ProviderCreationError,
    ProviderError,
};
use qubit_spi::{
    ProviderDefinition,
    ProviderDescriptor,
    ProviderId,
    ServiceProvider,
};

use crate::{
    FileCommandMimeDetector,
    MimeConfig,
    MimeDetector,
};

use super::MimeDetectorSpec;

/// Provider for the built-in system `file` command detector.
#[derive(Debug, Clone, Copy, Default)]
pub struct FileCommandMimeDetectorProvider;

impl ServiceProvider<MimeDetectorSpec> for FileCommandMimeDetectorProvider {
    fn create(
        &self,
        config: &MimeConfig,
    ) -> Result<Arc<dyn MimeDetector>, ProviderCreationError> {
        if !FileCommandMimeDetector::is_available() {
            return Err(ProviderError::unavailable(
                "`file` command is not available",
            )
            .into());
        }
        Ok(Arc::new(FileCommandMimeDetector::from_mime_config(
            config.clone(),
        )))
    }
}

impl ProviderDefinition<MimeDetectorSpec> for FileCommandMimeDetectorProvider {
    /// Returns the stable identity and automatic-selection priority.
    ///
    /// # Returns
    ///
    /// The `file` provider descriptor and its accepted aliases.
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor::new(
            ProviderId::new("file")
                .expect("built-in provider ID should be valid"),
        )
        .with_aliases(["file-command", "file-command-mime-detector"])
        .expect("built-in file detector aliases should be valid")
        .with_priority(10)
    }
}
