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
};
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
    ) -> Result<Arc<dyn MimeDetector>, ProviderCreationError> {
        match self.behavior {
            TestProviderBehavior::Success(mime_type) => {
                Ok(Arc::new(StaticMimeDetector::new(mime_type)))
            }
            TestProviderBehavior::Unsupported => {
                Err(ProviderError::unsupported("unsupported input").into())
            }
            TestProviderBehavior::Unavailable => {
                Err(ProviderError::unavailable("missing executable").into())
            }
            TestProviderBehavior::InitializationFailed => {
                Err(ProviderError::initialization_failed("startup failed")
                    .into())
            }
        }
    }
}

impl ProviderDefinition<MimeDetectorSpec> for TestMimeDetectorProvider {
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
