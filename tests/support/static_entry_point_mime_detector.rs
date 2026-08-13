// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::Path;

use qubit_io::std_io::ReadSeek;
use qubit_mime::{MimeDetectionPolicy, MimeDetector, MimeResult};

/// Detector fixture returning a distinct value from each trait entry point.
#[derive(Debug)]
pub(crate) struct StaticEntryPointMimeDetector;

impl MimeDetector for StaticEntryPointMimeDetector {
    fn max_buffer_size(&self) -> usize {
        0
    }

    fn detect_by_filename(&self, _filename: &str) -> MimeResult<Option<String>> {
        Ok(Some("application/x-static-name".to_owned()))
    }

    fn detect_by_content(&self, _content: &[u8]) -> MimeResult<Option<String>> {
        Ok(Some("application/x-static-content".to_owned()))
    }

    fn detect(
        &self,
        _content: &[u8],
        _filename: Option<&str>,
        _policy: MimeDetectionPolicy,
    ) -> MimeResult<Option<String>> {
        Ok(Some("application/x-static-detect".to_owned()))
    }

    fn detect_reader(
        &self,
        _reader: &mut dyn ReadSeek,
        _filename: Option<&str>,
        _policy: MimeDetectionPolicy,
    ) -> MimeResult<Option<String>> {
        Ok(Some("application/x-static-reader".to_owned()))
    }

    fn detect_file(
        &self,
        _file: &Path,
        _policy: MimeDetectionPolicy,
    ) -> MimeResult<Option<String>> {
        Ok(Some("application/x-static-file".to_owned()))
    }
}
