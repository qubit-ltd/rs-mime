// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::Arc;

use qubit_mime::{
    MimeConfig,
    MimeDetector,
    MimeDetectorSpec,
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
    StaticMimeDetector,
    TestProviderBehavior,
};

/// Self-described MIME detector provider used by Registry integration tests.
#[derive(Debug)]
pub(crate) struct TestMimeDetectorProvider {
    /// Canonical provider ID.
    id: &'static str,
    /// Aliases registered with the provider ID.
    aliases: &'static [&'static str],
    /// Automatic-selection priority.
    priority: i32,
    /// Creation behavior exercised by a test.
    behavior: TestProviderBehavior,
}

impl TestMimeDetectorProvider {
    /// Creates a provider fixture with no aliases.
    ///
    /// # Parameters
    ///
    /// * `id` - Canonical test provider ID.
    /// * `priority` - Automatic-selection priority.
    /// * `behavior` - Creation behavior for the provider.
    ///
    /// # Returns
    ///
    /// A self-described provider fixture.
    #[inline]
    pub(crate) const fn new(
        id: &'static str,
        priority: i32,
        behavior: TestProviderBehavior,
    ) -> Self {
        Self {
            id,
            aliases: &[],
            priority,
            behavior,
        }
    }

    /// Adds aliases to the provider fixture.
    ///
    /// # Parameters
    ///
    /// * `aliases` - Static aliases exposed through the descriptor.
    ///
    /// # Returns
    ///
    /// This provider with the supplied aliases.
    #[inline(always)]
    pub(crate) const fn with_aliases(
        mut self,
        aliases: &'static [&'static str],
    ) -> Self {
        self.aliases = aliases;
        self
    }
}

impl ServiceProvider<MimeDetectorSpec> for TestMimeDetectorProvider {
    fn create_configured(
        &self,
        _config: &MimeConfig,
    ) -> Result<Arc<dyn MimeDetector>, ProviderFailure<MimeError>> {
        match self.behavior {
            TestProviderBehavior::Success(mime_type) => {
                Ok(Arc::new(StaticMimeDetector::new(mime_type)))
            }
            TestProviderBehavior::Unsupported => {
                Err(ProviderFailure::unsupported(MimeError::DetectorBackend {
                    backend: "test".to_owned(),
                    reason: "unsupported input".to_owned(),
                }))
            }
            TestProviderBehavior::Unavailable => Err(
                ProviderFailure::unavailable(MimeError::DetectorUnavailable {
                    name: "test".to_owned(),
                    reason: "missing executable".to_owned(),
                }),
            ),
            TestProviderBehavior::InitializationFailed => {
                Err(ProviderFailure::initialization_failed(
                    MimeError::DetectorBackend {
                        backend: "test".to_owned(),
                        reason: "startup failed".to_owned(),
                    },
                ))
            }
        }
    }
}

impl ProviderMetadata for TestMimeDetectorProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor::new(
            ProviderId::new(self.id)
                .expect("test provider ID should be canonical"),
        )
        .with_aliases(self.aliases.iter().copied())
        .expect("test provider aliases should be valid")
        .with_priority(self.priority)
    }
}
