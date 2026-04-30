/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
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
