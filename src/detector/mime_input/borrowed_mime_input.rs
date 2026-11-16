// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Borrowed input views used while a MIME backend call is active.

use qubit_fs::AsyncFileSystem;
use qubit_fs::Path as FsPath;

use super::InputExtent;

/// Borrowed view valid only while the backend call is active.
#[derive(Clone, Copy)]
pub enum BorrowedMimeInput<'a> {
    /// Borrowed byte content and its explicit extent.
    Bytes {
        /// Borrowed byte content.
        content: &'a [u8],
        /// Whether the bytes are a prefix or the complete input.
        extent: InputExtent,
        /// Optional filename hint for detection.
        filename: Option<&'a str>,
    },
    /// Borrowed provider and path handles.
    Provider {
        /// Borrowed filesystem handle for provider-backed reads.
        file_system: &'a AsyncFileSystem,
        /// Provider path to inspect.
        path: &'a FsPath,
    },
}
