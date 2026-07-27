// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Provider for the built-in `ffprobe` media stream classifier.

use std::sync::Arc;

use qubit_spi::error::ProviderFailure;
use qubit_spi::{
    ProviderDescriptor,
    ProviderMetadata,
    ServiceProvider,
    provider_descriptor,
};

use crate::{
    FfprobeCommandMediaStreamClassifier,
    MediaStreamClassifier,
    MimeConfig,
    MimeError,
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
    /// Returns [`ProviderFailure`] classified as unavailable when the
    /// `ffprobe` command cannot be executed.
    #[inline]
    fn create_configured(
        &self,
        config: &MimeConfig,
    ) -> Result<Arc<dyn MediaStreamClassifier>, ProviderFailure<MimeError>>
    {
        if !FfprobeCommandMediaStreamClassifier::is_available() {
            return Err(ProviderFailure::unavailable(
                MimeError::ClassifierUnavailable {
                    name: "ffprobe".to_owned(),
                    reason: "`ffprobe` command is not available".to_owned(),
                },
            ));
        }
        Ok(Arc::new(
            FfprobeCommandMediaStreamClassifier::from_mime_config(
                config.clone(),
            ),
        ))
    }
}

impl ProviderMetadata for FfprobeCommandMediaStreamClassifierProvider {
    /// Returns the stable FFprobe provider identity and priority.
    ///
    /// # Returns
    ///
    /// The `ffprobe` descriptor and its accepted aliases.
    fn descriptor(&self) -> ProviderDescriptor {
        provider_descriptor!(
            "ffprobe",
            aliases: ["ffprobe-command", "ffprobe-command-media-stream-classifier"],
            priority: 10,
        )
    }
}
