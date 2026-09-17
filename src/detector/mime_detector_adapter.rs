// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Adapter from detector backends to the content backend contract.

use super::content_requirement::ContentRequirement;
use super::mime_content_backend::MimeContentBackend;
use crate::MimeDetectorBackend;
use crate::MimeResult;

/// Adapter exposing an existing detector backend through the content contract.
///
/// # Examples
///
/// ```
/// use qubit_mime::{MimeContentBackend, MimeDetectorAdapter, RepositoryMimeDetector};
/// let backend = RepositoryMimeDetector::new()?;
/// let adapter = MimeDetectorAdapter::new(backend);
/// assert!(!adapter.detect_bytes(b"%PDF-1.7")?.is_empty());
/// # Ok::<(), qubit_mime::MimeError>(())
/// ```
#[derive(Debug)]
pub struct MimeDetectorAdapter<B> {
    backend: B,
}

impl<B> MimeDetectorAdapter<B> {
    /// Wraps a detector backend.
    pub fn new(backend: B) -> Self {
        Self { backend }
    }

    /// Returns the wrapped backend.
    pub fn backend(&self) -> &B {
        &self.backend
    }
}

impl<B> MimeContentBackend for MimeDetectorAdapter<B>
where
    B: MimeDetectorBackend,
{
    /// Delegates the backend's input-size requirement.
    fn content_requirement(&self) -> ContentRequirement {
        self.backend.content_requirement()
    }

    /// Delegates content guessing to the wrapped backend.
    fn detect_bytes(&self, bytes: &[u8]) -> MimeResult<Vec<String>> {
        self.backend.guess_from_content(bytes)
    }
}
