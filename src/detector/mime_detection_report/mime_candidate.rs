// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================

/// A ranked MIME candidate. The rank is backend-defined and lower ranks win.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MimeCandidate {
    pub mime_type: String,
    pub rank: u32,
}
