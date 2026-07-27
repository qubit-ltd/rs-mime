// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::Arc;

use qubit_mime::{
    MediaStreamClassifier,
    MediaStreamClassifierSpec,
    MimeConfig,
    MimeError,
};
use qubit_spi::error::ProviderFailure;
use qubit_spi::{
    ProviderDescriptor,
    ProviderId,
    ProviderMetadata,
    ServiceProvider,
};

use super::{
    StaticMediaStreamClassifier,
    TestProviderBehavior,
};

/// Self-described classifier provider used by Registry integration tests.
#[derive(Debug)]
pub(crate) struct TestMediaStreamClassifierProvider {
    /// Canonical provider ID.
    id: &'static str,
    /// Automatic-selection priority.
    priority: i32,
    /// Creation behavior exercised by a test.
    behavior: TestProviderBehavior,
}

impl TestMediaStreamClassifierProvider {
    /// Creates a classifier provider fixture.
    ///
    /// # Parameters
    ///
    /// * `id` - Canonical test provider ID.
    /// * `priority` - Automatic-selection priority.
    /// * `behavior` - Creation behavior for the provider.
    ///
    /// # Returns
    ///
    /// A self-described classifier provider.
    #[inline]
    pub(crate) const fn new(
        id: &'static str,
        priority: i32,
        behavior: TestProviderBehavior,
    ) -> Self {
        Self {
            id,
            priority,
            behavior,
        }
    }
}

impl ServiceProvider<MediaStreamClassifierSpec>
    for TestMediaStreamClassifierProvider
{
    fn create_configured(
        &self,
        _config: &MimeConfig,
    ) -> Result<Arc<dyn MediaStreamClassifier>, ProviderFailure<MimeError>> {
        match self.behavior {
            TestProviderBehavior::Success(_) => {
                Ok(Arc::new(StaticMediaStreamClassifier))
            }
            TestProviderBehavior::Unsupported => {
                Err(ProviderFailure::unsupported(MimeError::ClassifierBackend {
                    backend: "test".to_owned(),
                    reason: "unsupported input".to_owned(),
                }))
            }
            TestProviderBehavior::Unavailable => {
                Err(ProviderFailure::unavailable(MimeError::ClassifierUnavailable {
                    name: "test".to_owned(),
                    reason: "missing executable".to_owned(),
                }))
            }
            TestProviderBehavior::InitializationFailed => {
                Err(ProviderFailure::initialization_failed(MimeError::ClassifierBackend {
                    backend: "test".to_owned(),
                    reason: "startup failed".to_owned(),
                }))
            }
        }
    }
}

impl ProviderMetadata for TestMediaStreamClassifierProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor::new(
            ProviderId::new(self.id)
                .expect("test provider ID should be canonical"),
        )
        .with_priority(self.priority)
    }
}
