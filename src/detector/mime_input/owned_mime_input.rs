// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Owned storage forms accepted by managed MIME detection.

use std::sync::Arc;

use qubit_fs::AsyncFileSystem;
use qubit_fs::Path as FsPath;

use super::InputExtent;

/// An input whose storage ownership transfers on successful submission.
pub enum OwnedMimeInput {
    /// In-memory bytes with an explicit range claim.
    Bytes {
        /// Shared byte storage transferred with the request.
        content: Arc<[u8]>,
        /// Whether the bytes are a prefix or the complete input.
        extent: InputExtent,
        /// Optional filename hint for detection.
        filename: Option<String>,
    },
    /// Provider-backed object, resolved once by the execution service.
    Provider {
        /// Shared filesystem handle for provider-backed reads.
        file_system: Arc<AsyncFileSystem>,
        /// Provider path to inspect.
        path: FsPath,
    },
}

impl OwnedMimeInput {
    /// Builds a byte input and records whether it is a prefix or complete
    /// object.
    ///
    /// # Parameters
    /// - `content`: Shared byte storage.
    /// - `extent`: Completeness claim for the stored bytes.
    ///
    /// # Returns
    /// An owned byte input without a filename hint.
    #[must_use]
    pub fn bytes(content: Arc<[u8]>, extent: InputExtent) -> Self {
        Self::Bytes {
            content,
            extent,
            filename: None,
        }
    }

    /// Adds a filename hint to byte input.
    ///
    /// # Parameters
    /// - `filename`: Filename hint converted into owned storage.
    ///
    /// # Returns
    /// The byte input with its filename hint, or the unchanged provider input.
    #[must_use]
    pub fn with_filename(self, filename: impl Into<String>) -> Self {
        match self {
            Self::Bytes { content, extent, .. } => Self::Bytes {
                content,
                extent,
                filename: Some(filename.into()),
            },
            provider => provider,
        }
    }
}
