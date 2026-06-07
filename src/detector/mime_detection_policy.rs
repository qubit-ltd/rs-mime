// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Policy for resolving combined MIME detection from filename and content.

/// Policy for resolving combined MIME detection from filename and content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MimeDetectionPolicy {
    /// Prefer a definitive filename result without checking content magic.
    PreferFilename,

    /// Check content magic even when filename detection has a definitive
    /// result.
    VerifyContent,
}
