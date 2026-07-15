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
//! implement [`ServiceProvider<MimeDetectorSpec>`](qubit_spi::ServiceProvider),
//! which creates detector instances. Provider identity, aliases, and priority
//! are supplied separately through a [`qubit_spi::ProviderDescriptor`] when an
//! application assembles a
//! [`MimeDetectorRegistry`](crate::MimeDetectorRegistry).

use qubit_spi::ServiceProvider;

use super::MimeDetectorSpec;

/// Marker trait for MIME detector providers.
///
/// Implement [`ServiceProvider<MimeDetectorSpec>`](qubit_spi::ServiceProvider)
/// for the concrete provider type. This marker keeps public registry bounds
/// MIME-specific while delegating provider behavior to `qubit-spi`.
pub trait MimeDetectorProvider: ServiceProvider<MimeDetectorSpec> {}

impl<T> MimeDetectorProvider for T where
    T: ServiceProvider<MimeDetectorSpec> + ?Sized
{
}
