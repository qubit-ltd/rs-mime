/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Provider contract for pluggable media stream classifiers.

use qubit_spi::ServiceProvider;

use super::MediaStreamClassifierSpec;

/// Marker trait for media stream classifier providers.
///
/// Implement [`ServiceProvider<MediaStreamClassifierSpec>`] for the concrete
/// provider type. This marker keeps public registry bounds MIME-specific while
/// delegating provider behavior to `qubit-spi`.
pub trait MediaStreamClassifierProvider: ServiceProvider<MediaStreamClassifierSpec> {}

impl<T> MediaStreamClassifierProvider for T where
    T: ServiceProvider<MediaStreamClassifierSpec> + ?Sized
{
}
