// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::Path;

use qubit_io::ReadSeek;
use qubit_mime::{
    MimeDetectionPolicy,
    MimeDetector,
    MimeResult,
};

/// MIME detector returning one test-controlled type for `.static` filenames.
#[derive(Debug)]
pub(crate) struct StaticMimeDetector {
    /// MIME type returned for matching filenames.
    mime_type: &'static str,
}

impl StaticMimeDetector {
    /// Creates a detector returning `mime_type` for `.static` filenames.
    ///
    /// # Parameters
    ///
    /// * `mime_type` - Static MIME type returned by matching detection calls.
    ///
    /// # Returns
    ///
    /// A deterministic detector fixture.
    #[inline]
    pub(crate) const fn new(mime_type: &'static str) -> Self {
        Self { mime_type }
    }
}

impl MimeDetector for StaticMimeDetector {
    fn detect_by_filename(&self, filename: &str) -> Option<String> {
        filename
            .ends_with(".static")
            .then(|| self.mime_type.to_owned())
    }

    fn detect_by_content(&self, _content: &[u8]) -> Option<String> {
        None
    }

    fn detect(
        &self,
        _content: &[u8],
        filename: Option<&str>,
        _policy: MimeDetectionPolicy,
    ) -> Option<String> {
        filename.and_then(|name| self.detect_by_filename(name))
    }

    fn detect_reader(
        &self,
        _reader: &mut dyn ReadSeek,
        filename: Option<&str>,
        policy: MimeDetectionPolicy,
    ) -> MimeResult<Option<String>> {
        Ok(self.detect(&[], filename, policy))
    }

    fn detect_file(
        &self,
        file: &Path,
        policy: MimeDetectionPolicy,
    ) -> MimeResult<Option<String>> {
        Ok(self.detect(
            &[],
            file.file_name().and_then(|name| name.to_str()),
            policy,
        ))
    }
}
