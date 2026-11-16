// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Owned and borrowed inputs for managed MIME detection.

use std::sync::Arc;

use qubit_fs::{AsyncFileSystem, Path as FsPath};

/// Completeness claim attached to byte input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputExtent {
    Prefix,
    Complete,
}

/// Portion of an input actually inspected by a backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InspectedExtent {
    Prefix(usize),
    Complete,
    RandomAccessSamples,
}

/// An input whose storage ownership transfers on successful submission.
pub enum OwnedMimeInput {
    /// In-memory bytes with an explicit range claim.
    Bytes {
        content: Arc<[u8]>,
        extent: InputExtent,
        filename: Option<String>,
    },
    /// Provider-backed object, resolved once by the execution service.
    Provider {
        file_system: Arc<AsyncFileSystem>,
        path: FsPath,
    },
}

impl OwnedMimeInput {
    /// Builds a byte input and records whether it is a prefix or complete object.
    #[must_use]
    pub fn bytes(content: Arc<[u8]>, extent: InputExtent) -> Self {
        Self::Bytes {
            content,
            extent,
            filename: None,
        }
    }

    /// Adds a filename hint to byte input.
    #[must_use]
    pub fn with_filename(self, filename: impl Into<String>) -> Self {
        match self {
            Self::Bytes {
                content, extent, ..
            } => Self::Bytes {
                content,
                extent,
                filename: Some(filename.into()),
            },
            provider => provider,
        }
    }
}

/// Borrowed view valid only while the backend call is active.
#[derive(Clone, Copy)]
pub enum BorrowedMimeInput<'a> {
    /// Borrowed byte content and its explicit extent.
    Bytes {
        content: &'a [u8],
        extent: InputExtent,
        filename: Option<&'a str>,
    },
    /// Borrowed provider and path handles.
    Provider {
        file_system: &'a AsyncFileSystem,
        path: &'a FsPath,
    },
}
