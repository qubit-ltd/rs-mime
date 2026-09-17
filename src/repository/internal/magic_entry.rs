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
    pub(crate) priority: u16,
    pub(crate) mime_index: usize,
    pub(crate) magic: MimeMagic,
}
