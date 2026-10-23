// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Result type used by MIME database parsing and detection.

use crate::MimeError;

/// The result type used by MIME-related functions.
///
/// # Type Parameters
/// - `T`: The value returned when the operation succeeds.
pub type MimeResult<T> = Result<T, MimeError>;
