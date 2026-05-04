/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Provider contract for pluggable MIME detector implementations.

use std::fmt::Debug;

use crate::{
    MimeConfig,
    MimeDetector,
    MimeResult,
};

use super::MimeDetectorAvailability;

/// Factory contract for MIME detector implementations.
pub trait MimeDetectorProvider: Debug + Send + Sync {
    /// Gets the canonical provider identifier.
    ///
    /// # Returns
    /// Stable lowercase provider identifier.
    fn id(&self) -> &'static str;

    /// Gets additional names accepted for this provider.
    ///
    /// # Returns
    /// Alias names. Matching is case-insensitive.
    fn aliases(&self) -> &'static [&'static str] {
        &[]
    }

    /// Gets provider priority used by `auto` selection.
    ///
    /// # Returns
    /// Larger values are preferred.
    fn priority(&self) -> i32 {
        0
    }

    /// Checks whether this provider can create a detector.
    ///
    /// # Parameters
    /// - `config`: MIME configuration used for provider-specific checks.
    ///
    /// # Returns
    /// Provider availability.
    fn availability(&self, _config: &MimeConfig) -> MimeDetectorAvailability {
        MimeDetectorAvailability::Available
    }

    /// Creates a detector instance.
    ///
    /// # Parameters
    /// - `config`: MIME configuration used to initialize the detector.
    ///
    /// # Returns
    /// Boxed detector implementation.
    ///
    /// # Errors
    /// Returns a [`MimeError`](crate::MimeError) when initialization fails.
    fn create(&self, config: &MimeConfig) -> MimeResult<Box<dyn MimeDetector>>;
}
