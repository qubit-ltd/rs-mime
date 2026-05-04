/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Availability state for MIME detector providers.

/// Availability of a detector provider in the current runtime environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MimeDetectorAvailability {
    /// The provider can create detectors.
    Available,
    /// The provider cannot create detectors.
    Unavailable {
        /// Human-readable reason.
        reason: String,
    },
}

impl MimeDetectorAvailability {
    /// Tells whether this availability state is available.
    ///
    /// # Returns
    /// `true` when the provider can create detectors.
    pub fn is_available(&self) -> bool {
        matches!(self, Self::Available)
    }
}
