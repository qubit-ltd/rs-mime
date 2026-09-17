// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Internal content magic index entries.

use super::super::MimeMagic;

#[derive(Debug, Clone)]
pub(crate) struct MagicEntry {
    /// Source priority of the magic rule.
    pub(crate) priority: u16,
    /// Index of the MIME type owning the rule.
    pub(crate) mime_index: usize,
    /// Parsed magic rule.
    pub(crate) magic: MimeMagic,
}
