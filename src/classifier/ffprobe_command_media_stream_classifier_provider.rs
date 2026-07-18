// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Provider for the built-in `ffprobe` media stream classifier.

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
    FfprobeCommandMediaStreamClassifier,
    MediaStreamClassifier,
    MimeConfig,
};

use super::MediaStreamClassifierSpec;

/// Provider for the built-in FFprobe-backed media stream classifier.
#[derive(Debug, Clone, Copy, Default)]
pub struct FfprobeCommandMediaStreamClassifierProvider;

impl ServiceProvider<MediaStreamClassifierSpec>
    for FfprobeCommandMediaStreamClassifierProvider
{
    /// Creates an FFprobe-backed classifier when the command is available.
    ///
    /// # Parameters
    ///
    /// * `config` - MIME configuration copied into the classifier.
    ///
    /// # Returns
    ///
    /// A shared FFprobe-backed media stream classifier.
    ///
    /// # Errors
    ///
    /// Returns [`ProviderCreationError`] classified as unavailable when the
    /// `ffprobe` command cannot be executed.
    #[inline]
    fn create_configured(
        &self,
        config: &MimeConfig,
    ) -> Result<Arc<dyn MediaStreamClassifier>, ProviderCreationError> {
        if !FfprobeCommandMediaStreamClassifier::is_available() {
            return Err(ProviderError::unavailable(
                "`ffprobe` command is not available",
            )
            .into());
        }
        Ok(Arc::new(
            FfprobeCommandMediaStreamClassifier::from_mime_config(
                config.clone(),
            ),
        ))
    }
}

impl ProviderDefinition<MediaStreamClassifierSpec>
    for FfprobeCommandMediaStreamClassifierProvider
{
    /// Returns the stable FFprobe provider identity and priority.
    ///
    /// # Returns
    ///
    /// The `ffprobe` descriptor and its accepted aliases.
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor::new(
            ProviderId::new("ffprobe")
                .expect("built-in provider ID should be valid"),
        )
        .with_aliases([
            "ffprobe-command",
            "ffprobe-command-media-stream-classifier",
        ])
        .expect("built-in FFprobe classifier aliases should be valid")
        .with_priority(10)
    }
}
