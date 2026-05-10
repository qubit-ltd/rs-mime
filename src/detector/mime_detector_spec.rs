/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Service specification for pluggable MIME detector providers.

use qubit_spi::ServiceSpec;

use crate::{
    MimeConfig,
    MimeDetector,
};

/// Service specification for pluggable MIME detector providers.
///
/// The configuration is [`MimeConfig`], and created services implement the
/// [`MimeDetector`] trait object.
#[derive(Debug)]
pub struct MimeDetectorSpec;

impl ServiceSpec for MimeDetectorSpec {
    type Config = MimeConfig;
    type Service = dyn MimeDetector;
}
