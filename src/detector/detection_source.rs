/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Source available for precise MIME refinement.

use std::path::Path;

/// Source available for precise MIME refinement.
#[derive(Debug, Clone, Copy)]
pub enum DetectionSource<'a> {
    /// No readable source is available.
    None,
    /// In-memory content bytes are available.
    Content(&'a [u8]),
    /// A local file path is available.
    Path(&'a Path),
}
