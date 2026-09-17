// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Internal filename glob index entries.

use super::super::MimeGlob;

#[derive(Debug, Clone)]
pub(crate) struct GlobEntry {
    pub(crate) glob: MimeGlob,
    pub(crate) mime_index: usize,
}
