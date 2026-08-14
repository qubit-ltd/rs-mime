// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Service specification for pluggable media stream classifier providers.

use std::sync::Arc;

use qubit_spi::ServiceSpec;
use qubit_spi::SyncServiceSpec;

use crate::MediaStreamClassifier;
use crate::MimeConfig;
use crate::MimeError;

/// Service specification for pluggable media stream classifier providers.
///
/// The configuration is [`MimeConfig`], and created services implement the
/// [`MediaStreamClassifier`] trait object.
#[derive(Debug)]
pub struct MediaStreamClassifierSpec;

impl ServiceSpec for MediaStreamClassifierSpec {
    type Config = MimeConfig;
    type Error = MimeError;
}

impl SyncServiceSpec for MediaStreamClassifierSpec {
    type Output = Arc<dyn MediaStreamClassifier>;
}
