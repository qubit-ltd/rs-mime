// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Provider for the built-in `ffprobe` media stream classifier.

use crate::{
    FfprobeCommandMediaStreamClassifier,
    MediaStreamClassifier,
    MimeConfig,
    ProviderAvailability,
    ProviderCreateError,
    ProviderDescriptor,
    ProviderRegistryError,
    ServiceProvider,
};

use super::{
    MediaStreamClassifierAvailability,
    MediaStreamClassifierSpec,
};

/// Provider for the built-in FFprobe-backed media stream classifier.
#[derive(Debug, Clone, Copy, Default)]
pub struct FfprobeCommandMediaStreamClassifierProvider;

impl ServiceProvider<MediaStreamClassifierSpec>
    for FfprobeCommandMediaStreamClassifierProvider
{
    /// Gets FFprobe classifier metadata.
    fn descriptor(&self) -> Result<ProviderDescriptor, ProviderRegistryError> {
        let descriptor = ProviderDescriptor::new("ffprobe")
            .expect("built-in FFprobe classifier provider id should be valid")
            .with_aliases(&[
                "ffprobe-command",
                "ffprobe-command-media-stream-classifier",
            ])
            .expect("built-in FFprobe classifier aliases should be valid")
            .with_priority(10);
        Ok(descriptor)
    }

    /// Reports the provider as available.
    ///
    /// The classifier itself handles command execution lazily so temporary
    /// `PATH` changes and best-effort refinement do not make registry creation
    /// environment-sensitive.
    fn availability(
        &self,
        _config: &MimeConfig,
    ) -> MediaStreamClassifierAvailability {
        ProviderAvailability::Available
    }

    /// Creates an FFprobe-backed classifier.
    fn create_box(
        &self,
        config: &MimeConfig,
    ) -> Result<Box<dyn MediaStreamClassifier>, ProviderCreateError> {
        Ok(Box::new(
            FfprobeCommandMediaStreamClassifier::from_mime_config(
                config.clone(),
            ),
        ))
    }
}
