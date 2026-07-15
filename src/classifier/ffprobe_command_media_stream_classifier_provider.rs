// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Provider for the built-in `ffprobe` media stream classifier.

use std::sync::Arc;

use qubit_spi::{
    ProviderDescriptor,
    ProviderError,
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
    fn create(
        &self,
        config: &MimeConfig,
    ) -> Result<Arc<dyn MediaStreamClassifier>, ProviderError> {
        Ok(Arc::new(
            FfprobeCommandMediaStreamClassifier::from_mime_config(
                config.clone(),
            ),
        ))
    }
}

/// Gets the immutable descriptor for the FFprobe classifier provider.
#[must_use]
pub fn ffprobe_command_media_stream_classifier_descriptor() -> ProviderDescriptor
{
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
