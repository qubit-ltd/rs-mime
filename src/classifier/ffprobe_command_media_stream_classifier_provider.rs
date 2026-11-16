// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Provider for the built-in `ffprobe` media stream classifier.

use std::sync::Arc;

use qubit_spi::ProviderDescriptor;
use qubit_spi::ProviderMetadata;
use qubit_spi::ServiceProvider;
use qubit_spi::error::ProviderFailure;
use qubit_spi::provider_descriptor;

use super::MediaStreamClassifierSpec;
use crate::AsyncMediaStreamClassifierBackend;
use crate::FfprobeCommandMediaStreamClassifier;
use crate::MediaStreamClassifier;
use crate::MimeConfig;
use crate::MimeError;
use crate::detector::MimeExecutionServices;

/// Provider for the built-in FFprobe-backed media stream classifier.
///
/// # Examples
///
/// ```rust
/// use qubit_mime::FfprobeCommandMediaStreamClassifierProvider;
/// use qubit_spi::ProviderMetadata;
///
/// let provider = FfprobeCommandMediaStreamClassifierProvider;
/// let descriptor = provider.descriptor();
/// assert_eq!("ffprobe", descriptor.id().as_str());
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct FfprobeCommandMediaStreamClassifierProvider;

impl ServiceProvider<MediaStreamClassifierSpec> for FfprobeCommandMediaStreamClassifierProvider {
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
    ) -> Result<Arc<dyn MediaStreamClassifier>, ProviderFailure<MimeError>> {
        if !FfprobeCommandMediaStreamClassifier::is_available_with_config(config) {
            return Err(ProviderFailure::unavailable(MimeError::ClassifierUnavailable {
                name: "ffprobe".to_owned(),
                reason: "`ffprobe` command is not available".to_owned(),
            }));
        }
        Ok(Arc::new(FfprobeCommandMediaStreamClassifier::from_mime_config(
            config.clone(),
        )))
    }
}

impl FfprobeCommandMediaStreamClassifierProvider {
    /// Creates an FFprobe classifier configured for explicit async operations.
    ///
    /// # Parameters
    /// - `config`: MIME configuration used to set classifier staging limits.
    /// - `services`: Shared execution services retained by the classifier.
    ///
    /// # Returns
    /// An asynchronous backend that uses the supplied operation services.
    pub(crate) fn create_async_configured(
        &self,
        config: &MimeConfig,
        services: Arc<MimeExecutionServices>,
    ) -> Arc<dyn AsyncMediaStreamClassifierBackend> {
        Arc::new(
            FfprobeCommandMediaStreamClassifier::from_mime_config(config.clone()).with_execution_services(services),
        )
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

impl super::AsyncMediaStreamClassifierProvider for FfprobeCommandMediaStreamClassifierProvider {
    /// Creates the FFprobe async backend for the supplied operation services.
    ///
    /// # Parameters
    /// - `config`: MIME limits used to configure the classifier.
    /// - `services`: Operation execution and staging services.
    ///
    /// # Returns
    /// The configured asynchronous classifier backend.
    ///
    /// # Errors
    /// This provider currently constructs without additional fallible work.
    fn create_async_backend(
        &self,
        config: &MimeConfig,
        services: &MimeExecutionServices,
    ) -> Result<Arc<dyn AsyncMediaStreamClassifierBackend>, MimeError> {
        Ok(self.create_async_configured(config, Arc::new(services.clone())))
    }
}
