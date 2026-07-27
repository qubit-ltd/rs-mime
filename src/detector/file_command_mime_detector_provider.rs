// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Provider for the built-in system `file` command MIME detector.

use std::sync::Arc;

use qubit_spi::error::ProviderFailure;
use qubit_spi::{
    ProviderDescriptor,
    ProviderMetadata,
    ServiceProvider,
    provider_descriptor,
};

use crate::{
    FileCommandMimeDetector,
    MimeConfig,
    MimeDetector,
    MimeError,
};

use super::MimeDetectorSpec;

/// Provider for the built-in system `file` command detector.
#[derive(Debug, Clone, Copy, Default)]
pub struct FileCommandMimeDetectorProvider;

impl ServiceProvider<MimeDetectorSpec> for FileCommandMimeDetectorProvider {
    fn create_configured(
        &self,
        config: &MimeConfig,
    ) -> Result<Arc<dyn MimeDetector>, ProviderFailure<MimeError>> {
        if !FileCommandMimeDetector::is_available() {
            return Err(ProviderFailure::unavailable(
                MimeError::DetectorUnavailable {
                    name: "file".to_owned(),
                    reason: "`file` command is not available".to_owned(),
                },
            ));
        }
        Ok(Arc::new(FileCommandMimeDetector::from_mime_config(
            config.clone(),
        )))
    }
}

impl ProviderMetadata for FileCommandMimeDetectorProvider {
    /// Returns the stable identity and automatic-selection priority.
    ///
    /// # Returns
    ///
    /// The `file` provider descriptor and its accepted aliases.
    fn descriptor(&self) -> ProviderDescriptor {
        provider_descriptor!(
            "file",
            aliases: ["file-command", "file-command-mime-detector"],
            priority: 10,
        )
    }
}
