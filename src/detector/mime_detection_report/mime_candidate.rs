// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
/// A ranked MIME candidate. The rank is backend-defined and lower ranks win.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MimeCandidate {
    /// Canonical MIME type represented by this candidate.
    pub mime_type: String,
    /// Backend-defined rank; lower values have higher priority.
    pub rank: u32,
}
