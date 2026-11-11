// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Helpers for stream-backed MIME detectors.

use std::fmt::Debug;
use std::fs::File;
use std::future::Future;
use std::io;
use std::io::ErrorKind;
use std::path::Path;
use std::pin::Pin;

use qubit_io::std_io::ReadSeek;
use qubit_io::std_io::ext::ReadSeekExt;

use crate::MimeDetectorCore;
use crate::MimeError;
use crate::MimeResult;

/// Core implementation contract for detectors that can inspect content bytes.
///
/// Implement this trait to inspect content streams through a MIME detector.
/// Values are supplied by the detector implementation or by the caller that
/// constructs it.
///
/// # Examples
///
/// ```
/// use qubit_mime::StreamBasedMimeDetector;
///
/// fn filename_candidates(detector: &dyn StreamBasedMimeDetector) -> Vec<String> {
///     detector.guess_from_filename("image.png")
/// }
/// ```
pub trait StreamBasedMimeDetector: Debug + Send + Sync {
    /// Gets the shared detector core.
    ///
    /// # Returns
    /// Shared detector configuration and merge/refinement behavior.
    #[must_use]
    fn core(&self) -> &MimeDetectorCore;

    /// Gets the maximum number of bytes needed for content inspection.
    ///
    /// # Returns
    /// Content prefix length to read from files and readers.
    #[must_use]
    fn max_test_bytes(&self) -> usize;

    /// Guesses MIME type names from a filename.
    ///
    /// # Parameters
    /// - `filename`: File path or basename.
    ///
    /// # Returns
    /// Candidate MIME type names ordered by backend confidence.
    #[must_use]
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
    fn guess_from_content_bytes(&self, content: &[u8]) -> MimeResult<Vec<String>>;

    /// Guesses candidates asynchronously, using the synchronous hook inline
    /// by default. Implementations that may block should override this method.
    ///
    /// # Parameters
    /// - `content`: Content bytes to inspect.
    ///
    /// # Returns
    /// A future containing candidate MIME type names.
    ///
    /// # Errors
    /// Propagates content-inspection errors.
    fn guess_from_content_bytes_async<'a>(
        &'a self,
        content: &'a [u8],
    ) -> Pin<Box<dyn Future<Output = MimeResult<Vec<String>>> + Send + 'a>> {
        Box::pin(async move { self.guess_from_content_bytes(content) })
    }

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
    fn guess_from_reader_stream(&self, reader: &mut dyn ReadSeek) -> MimeResult<(Vec<String>, Vec<u8>)> {
        let content = read_prefix(reader, self.max_test_bytes(), self.core().max_buffer_size())?;
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
    fn guess_from_file_stream(&self, file: &Path) -> MimeResult<(Vec<String>, Vec<u8>)> {
        let mut reader = open_readable_file(file)?;
        self.guess_from_reader_stream(&mut reader)
    }
}

/// Opens a local regular file for detector reads.
///
/// # Parameters
/// - `path`: Local path to validate and open for reading.
///
/// # Errors
/// Returns an invalid-input error when `path` is not a regular file, or the
/// operating-system error raised while reading metadata or opening the file.
pub(crate) fn open_readable_file(path: &Path) -> io::Result<File> {
    if !std::fs::metadata(path)?.is_file() {
        return Err(io::Error::new(ErrorKind::InvalidInput, "path is not a regular file"));
    }
    File::open(path)
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
pub(crate) fn read_prefix(reader: &mut dyn ReadSeek, max_bytes: usize, max_buffer_size: usize) -> MimeResult<Vec<u8>> {
    if max_bytes > max_buffer_size {
        return Err(MimeError::BufferLimitExceeded {
            requested: max_bytes,
            limit: max_buffer_size,
        });
    }
    let mut buffer = Vec::new();
    buffer
        .try_reserve_exact(max_bytes)
        .map_err(|source| MimeError::BufferAllocationFailed {
            requested: max_bytes,
            source,
        })?;
    buffer.resize(max_bytes, 0);
    let bytes_read = reader.peek_exact_or_eof(&mut buffer)?;
    buffer.truncate(bytes_read);
    Ok(buffer)
}
