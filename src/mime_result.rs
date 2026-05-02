/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Result type used by MIME database parsing and detection.
//!

use crate::MimeError;

/// A convenient alias for results returned by MIME-related functions.
pub type MimeResult<T> = Result<T, MimeError>;
