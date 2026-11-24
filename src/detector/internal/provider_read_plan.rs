// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Immutable provider-read strategy captured before submission locking.

use crate::ContentRequirement;
use crate::RandomAccessCapability;
use crate::detector::AsyncMimeDetectorBackend;

/// Immutable provider-read strategy captured once before submission locking.
#[derive(Debug, Clone, Copy)]
pub(in crate::detector) struct ProviderReadPlan {
    /// Minimum content extent declared by the backend.
    pub requirement: ContentRequirement,
    /// Same-object random-access capability declared by the backend.
    pub random_access: RandomAccessCapability,
    /// Whether provider content is exposed only through a metered session.
    pub managed_provider_reads: bool,
}

impl ProviderReadPlan {
    /// Captures extension capabilities once and enforces managed reads for
    /// complete and same-object sampling backends.
    ///
    /// # Parameters
    /// - `backend`: Backend whose content capabilities are captured.
    /// - `trusted_self_metered_file`: Whether the input is already metered.
    ///
    /// # Returns
    /// A read plan enforcing the backend's declared access requirements.
    pub(in crate::detector) fn capture(
        backend: &dyn AsyncMimeDetectorBackend,
        trusted_self_metered_file: bool,
    ) -> Self {
        let requirement = backend.content_requirement();
        let random_access = backend.random_access_capability();
        let requested_managed_reads = backend.uses_managed_provider_reads();
        let managed_provider_reads = requested_managed_reads
            || (matches!(requirement, ContentRequirement::Complete) && !trusted_self_metered_file)
            || random_access == RandomAccessCapability::SamplesSameObject;
        Self {
            requirement,
            random_access,
            managed_provider_reads,
        }
    }

    /// Returns the extent needed after filename short-circuiting.
    ///
    /// # Parameters
    /// - `filename_short_circuit`: Whether filename rules already resolved
    ///   input.
    ///
    /// # Returns
    /// The content extent required for submission.
    pub(in crate::detector) fn requirement_for(self, filename_short_circuit: bool) -> ContentRequirement {
        if filename_short_circuit {
            ContentRequirement::Prefix(0)
        } else {
            self.requirement
        }
    }
}
