// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Helpers for stream-backed MIME detectors.

use std::fmt::Debug;
use std::path::Path;

use qubit_io::{
    ReadSeek,
    ReadSeekExt,
};
use qubit_local_files::read;

use crate::{
    MimeDetectorCore,
    MimeError,
    MimeResult,
};

/// Core implementation contract for detectors that can inspect content bytes.
pub trait StreamBasedMimeDetector: Debug + Send + Sync {
    /// Gets the shared detector core.
    ///
    /// # Returns
    /// Shared detector configuration and merge/refinement behavior.
    fn core(&self) -> &MimeDetectorCore;

    /// Gets the maximum number of bytes needed for content inspection.
    ///
    /// # Returns
    /// Content prefix length to read from files and readers.
    fn max_test_bytes(&self) -> usize;

    /// Guesses MIME type names from a filename.
    ///
    /// # Parameters
    /// - `filename`: File path or basename.
    ///
    /// # Returns
    /// Candidate MIME type names ordered by backend confidence.
    fn guess_from_filename(&self, filename: &str) -> Vec<String>;

    /// Guesses MIME type names from content bytes.
    ///
    /// # Parameters
    /// - `content`: Content bytes.
    ///
    /// # Returns
    /// Candidate MIME type names ordered by backend confidence.
    ///
    /// # Errors
    /// Returns an error when a backend cannot inspect the supplied content.
    fn guess_from_content_bytes(
        &self,
        content: &[u8],
    ) -> MimeResult<Vec<String>>;

    /// Guesses MIME type names from a seekable reader.
    ///
    /// # Parameters
    /// - `reader`: Reader to inspect. The original position is restored.
    ///
    /// # Returns
    /// Candidate MIME type names and the content prefix used for refinement.
    ///
    /// # Errors
    /// Returns an error when reading, seeking, or backend inspection fails.
    fn guess_from_reader_stream(
        &self,
        reader: &mut dyn ReadSeek,
    ) -> MimeResult<(Vec<String>, Vec<u8>)> {
        let content = read_prefix(
            reader,
            self.max_test_bytes(),
            self.core().max_buffer_size(),
        )?;
        let candidates = self.guess_from_content_bytes(&content)?;
        Ok((candidates, content))
    }

    /// Guesses MIME type names from a local file.
    ///
    /// # Parameters
    /// - `file`: Local file path.
    ///
    /// # Returns
    /// Candidate MIME type names and the content prefix used for refinement.
    ///
    /// # Errors
    /// Returns an error when opening, reading, seeking, or backend inspection
    /// fails.
    fn guess_from_file_stream(
        &self,
        file: &Path,
    ) -> MimeResult<(Vec<String>, Vec<u8>)> {
        let mut reader = read::open(file)?;
        self.guess_from_reader_stream(&mut reader)
    }
}

/// Reads a prefix from a stream and restores the original position.
///
/// # Parameters
/// - `reader`: Stream to inspect.
/// - `max_bytes`: Maximum number of bytes to read.
/// - `max_buffer_size`: Maximum allowed byte buffer allocation.
///
/// # Returns
/// Bytes read from the stream.
///
/// # Errors
/// Returns [`MimeError::BufferLimitExceeded`](crate::MimeError::BufferLimitExceeded) when
/// `max_bytes` exceeds `max_buffer_size`. Returns
/// [`MimeError::Io`](crate::MimeError::Io) when reading or seeking fails.
pub(crate) fn read_prefix(
    reader: &mut dyn ReadSeek,
    max_bytes: usize,
    max_buffer_size: usize,
) -> MimeResult<Vec<u8>> {
    if max_bytes > max_buffer_size {
        return Err(MimeError::BufferLimitExceeded {
            requested: max_bytes,
            limit: max_buffer_size,
        });
    }
    let mut buffer = vec![0; max_bytes];
    let bytes_read = reader.peek_exact_or_eof(&mut buffer)?;
    buffer.truncate(bytes_read);
    Ok(buffer)
}
