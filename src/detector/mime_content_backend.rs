// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Object-safe backend contract for MIME content providers.

use std::fmt::Debug;
use std::io::Read;

use qubit_io::std_io::ReadSeek;

pub use super::content_requirement::ContentRequirement;
pub use super::mime_detector_adapter::MimeDetectorAdapter;
use crate::MimeResult;

/// Object-safe contract for content-based MIME providers.
pub trait MimeContentBackend: Debug + Send + Sync {
    /// Declares the amount of input required by this backend.
    fn content_requirement(&self) -> ContentRequirement;

    /// Detects MIME candidates from bytes.
    fn detect_bytes(&self, bytes: &[u8]) -> MimeResult<Vec<String>>;

    /// Detects MIME candidates from a seekable reader.
    fn detect_reader(&self, reader: &mut dyn ReadSeek) -> MimeResult<Vec<String>> {
        let position = reader.stream_position()?;
        let mut bytes = Vec::new();
        match self.content_requirement() {
            ContentRequirement::Prefix(limit) => {
                reader.take(limit as u64).read_to_end(&mut bytes)?;
            }
            ContentRequirement::Complete => {
                reader.read_to_end(&mut bytes)?;
            }
        }
        reader.seek(std::io::SeekFrom::Start(position))?;
        self.detect_bytes(&bytes)
    }
}
