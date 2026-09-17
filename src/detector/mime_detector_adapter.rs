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
    fn content_requirement(&self) -> ContentRequirement {
        self.backend.content_requirement()
    }

    fn detect_bytes(&self, bytes: &[u8]) -> MimeResult<Vec<String>> {
        self.backend.guess_from_content(bytes)
    }
}
