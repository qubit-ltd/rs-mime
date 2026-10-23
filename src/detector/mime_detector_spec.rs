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
///
/// # Examples
///
/// ```
/// use std::sync::Arc;
///
/// use qubit_mime::MimeConfig;
/// use qubit_mime::MimeDetector;
/// use qubit_mime::MimeDetectorSpec;
/// use qubit_mime::MimeError;
/// use qubit_spi::ServiceSpec;
/// use qubit_spi::SyncServiceSpec;
///
/// fn assert_config<T: ServiceSpec<Config = MimeConfig, Error = MimeError>>() {}
/// fn assert_output<T: SyncServiceSpec<Output = Arc<dyn MimeDetector>>>() {}
///
/// assert_config::<MimeDetectorSpec>();
/// assert_output::<MimeDetectorSpec>();
/// ```
#[derive(Debug)]
pub struct MimeDetectorSpec;

impl ServiceSpec for MimeDetectorSpec {
    /// Configuration accepted by this service specification.
    type Config = MimeConfig;
    /// Error returned while creating this service.
    type Error = MimeError;
}

impl SyncServiceSpec for MimeDetectorSpec {
    /// Shared detector service produced by providers.
    type Output = Arc<dyn MimeDetector>;
}
