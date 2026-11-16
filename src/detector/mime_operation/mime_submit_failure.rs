// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Failed submission data that preserves ownership of the original input.

use super::MimeSubmitRejection;
use crate::OwnedMimeInput;

/// Failed submission with the original input returned to its caller.
pub struct MimeSubmitFailure {
    /// Reason the body was rejected before it started.
    pub reason: MimeSubmitRejection,
    /// Input returned intact for retry or alternate handling.
    pub input: OwnedMimeInput,
}
