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
///
/// # Examples
///
/// ```
/// use qubit_mime::{ContentRequirement, MimeContentBackend, MimeResult};
///
/// #[derive(Debug)]
/// struct PdfBackend;
///
/// impl MimeContentBackend for PdfBackend {
///     fn content_requirement(&self) -> ContentRequirement {
///         ContentRequirement::Prefix(5)
///     }
///
///     fn detect_bytes(&self, bytes: &[u8]) -> MimeResult<Vec<String>> {
///         let mime_types = if bytes.starts_with(b"%PDF-") {
///             vec!["application/pdf".to_owned()]
///         } else {
///             Vec::new()
///         };
///         Ok(mime_types)
///     }
/// }
///
/// let backend = PdfBackend;
/// assert_eq!(
///     backend.detect_bytes(b"%PDF-1.7").unwrap(),
///     vec!["application/pdf".to_owned()],
/// );
/// ```
pub trait MimeContentBackend: Debug + Send + Sync {
    /// Returns how many bytes this backend needs to inspect.
    ///
    /// A prefix requirement allows callers to bound stream reads; a complete
    /// requirement asks callers to provide all available content.
    fn content_requirement(&self) -> ContentRequirement;

    /// Returns MIME candidates identified from the supplied content bytes.
    fn detect_bytes(&self, bytes: &[u8]) -> MimeResult<Vec<String>>;

    /// Reads the required content from a seekable reader and detects MIME
    /// candidates from those bytes.
    ///
    /// When the read succeeds, the reader is returned to its original position
    /// before detection. I/O errors are returned if reading or seeking fails.
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
