// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Provider contract for pluggable media stream classifiers.

use qubit_spi::ProviderDefinition;
use std::sync::Arc;

use super::MediaStreamClassifierSpec;
use crate::{AsyncMediaStreamClassifierBackend, MimeConfig, MimeError, MimeExecutionServices};

/// Marker trait for media stream classifier providers.
///
/// Implement [`ProviderDefinition<MediaStreamClassifierSpec>`] for the
/// concrete provider type. This marker ensures every registrable classifier
/// supplies both creation behavior and registration metadata.
///
/// # Examples
///
/// ```
/// use qubit_mime::FfprobeCommandMediaStreamClassifierProvider;
/// use qubit_mime::MediaStreamClassifierProvider;
///
/// fn assert_provider<T: MediaStreamClassifierProvider>() {}
///
/// assert_provider::<FfprobeCommandMediaStreamClassifierProvider>();
/// ```
pub trait MediaStreamClassifierProvider: ProviderDefinition<MediaStreamClassifierSpec> {
    /// Constructs the provider's explicit async classifier. Sync-only
    /// providers reject this operation without invoking synchronous
    /// construction or classification.
    fn create_operation_backend(
        &self,
        _config: &MimeConfig,
        _services: MimeExecutionServices,
    ) -> Result<Arc<dyn AsyncMediaStreamClassifierBackend>, MimeError> {
        Err(MimeError::AsyncUnsupported)
    }
}

impl<T> MediaStreamClassifierProvider for T where
    T: ProviderDefinition<MediaStreamClassifierSpec> + ?Sized
{
}
