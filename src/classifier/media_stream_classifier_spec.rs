// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Service specification for pluggable media stream classifier providers.

use qubit_spi::ServiceSpec;

use crate::{
    MediaStreamClassifier,
    MimeConfig,
};

/// Service specification for pluggable media stream classifier providers.
///
/// The configuration is [`MimeConfig`], and created services implement the
/// [`MediaStreamClassifier`] trait object.
#[derive(Debug)]
pub struct MediaStreamClassifierSpec;

impl ServiceSpec for MediaStreamClassifierSpec {
    type Config = MimeConfig;
    type Service = dyn MediaStreamClassifier;
}
