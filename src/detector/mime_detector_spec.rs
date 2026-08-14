// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Service specification for pluggable MIME detector providers.

use std::sync::Arc;

use qubit_spi::ServiceSpec;
use qubit_spi::SyncServiceSpec;

use crate::MimeConfig;
use crate::MimeDetector;
use crate::MimeError;

/// Service specification for pluggable MIME detector providers.
///
/// The configuration is [`MimeConfig`], and created services implement the
/// [`MimeDetector`] trait object.
#[derive(Debug)]
pub struct MimeDetectorSpec;

impl ServiceSpec for MimeDetectorSpec {
    type Config = MimeConfig;
    type Error = MimeError;
}

impl SyncServiceSpec for MimeDetectorSpec {
    type Output = Arc<dyn MimeDetector>;
}
