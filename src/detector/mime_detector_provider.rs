// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Provider contract for pluggable MIME detector implementations.
//!
//! The MIME detector SPI is a domain binding over [`qubit_spi`]. Providers
//! implement [`qubit_spi::ProviderDefinition<MimeDetectorSpec>`], which
//! combines detector creation with self-described registration metadata.

use qubit_spi::ProviderDefinition;

use super::MimeDetectorSpec;

/// Marker trait for MIME detector providers.
///
/// Implement [`ProviderDefinition<MimeDetectorSpec>`] for the concrete
/// provider type. This marker keeps public registry bounds MIME-specific while
/// ensuring every registrable detector supplies its own descriptor.
pub trait MimeDetectorProvider: ProviderDefinition<MimeDetectorSpec> {}

impl<T> MimeDetectorProvider for T where T: ProviderDefinition<MimeDetectorSpec> + ?Sized {}
