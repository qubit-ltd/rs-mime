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
use crate::MeteredReadSession;

/// Borrowed view valid only while the backend call is active.
///
/// Provider-backed extension backends should handle
/// [`BorrowedMimeInput::ManagedProvider`] and read through its session. That
/// variant carries the runtime's already-opened object and is the only provider
/// input exposed to managed third-party backends. The enum is no longer `Copy`
/// or `Clone` because a managed session is mutably borrowed for one backend
/// call.
///
/// ```ignore
/// match input {
///     BorrowedMimeInput::ManagedProvider { session, .. } => {
///         let mut prefix = [0; 16];
///         let count = session.read(&mut prefix).await?;
///         inspect(&prefix[..count])
///     }
///     BorrowedMimeInput::Bytes { content, .. } => inspect(content),
///     BorrowedMimeInput::Provider { .. } => return Err(unmanaged_provider_input()),
/// }
/// ```
///
/// # Examples
///
/// ```
/// use qubit_mime::{BorrowedMimeInput, InputExtent};
///
/// let content = b"media";
/// let input = BorrowedMimeInput::Bytes {
///     content,
///     extent: InputExtent::Complete,
///     filename: Some("clip.mp4"),
/// };
/// assert!(matches!(input, BorrowedMimeInput::Bytes { filename: Some("clip.mp4"), .. }));
/// ```
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
    /// Managed provider object exposed only through metered reads.
    ///
    /// Use `session.read` for sequential reads and `session.read_at` for
    /// offsets when the backend declared same-object random access. Do not
    /// reopen the original path or access a raw filesystem handle.
    ManagedProvider {
        /// Read session for one already-opened provider object.
        session: &'a mut MeteredReadSession,
        /// Optional filename hint for detection.
        filename: Option<&'a str>,
    },
}
