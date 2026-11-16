// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Adapter from detector backends to the content backend contract.

use super::content_requirement::ContentRequirement;
use super::internal::remaining_content_reader::RemainingContentReader;
use super::mime_content_backend::MimeContentBackend;
use crate::MimeDetectorBackend;
use crate::MimeResult;

/// Adapter exposing an existing detector backend through the content contract.
///
/// # Type Parameters
///
/// * `B` - Detector backend wrapped by this adapter.
///
/// # Examples
///
/// ```
/// use qubit_mime::MimeContentBackend;
/// use qubit_mime::MimeDetectorAdapter;
/// use qubit_mime::RepositoryMimeDetector;
/// let backend = RepositoryMimeDetector::new()?;
/// let adapter = MimeDetectorAdapter::new(backend);
/// assert!(!adapter.detect_bytes(b"%PDF-1.7")?.is_empty());
/// # Ok::<(), qubit_mime::MimeError>(())
/// ```
#[derive(Debug)]
pub struct MimeDetectorAdapter<B> {
    /// Detector backend wrapped by this adapter.
    backend: B,
}

impl<B> MimeDetectorAdapter<B> {
    /// Wraps a detector backend.
    ///
    /// # Parameters
    ///
    /// * `backend` - Detector backend to expose through the content contract.
    ///
    /// # Returns
    ///
    /// An adapter that exposes `backend` through the content backend contract.
    #[must_use]
    #[inline]
    pub fn new(backend: B) -> Self {
        Self { backend }
    }

    /// Returns the wrapped backend.
    ///
    /// # Returns
    ///
    /// A shared reference to the backend owned by this adapter.
    #[must_use]
    #[inline]
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

    /// Delegates the reader buffer budget to the wrapped detector
    /// configuration.
    fn max_reader_buffer_size(&self) -> usize {
        self.backend.core().max_buffer_size()
    }

    /// Delegates content guessing to the wrapped backend.
    ///
    /// # Parameters
    ///
    /// * `bytes` - Content bytes to inspect.
    fn detect_bytes(&self, bytes: &[u8]) -> MimeResult<Vec<String>> {
        self.backend.guess_from_content(bytes)
    }

    /// Delegates reader detection using a view beginning at the current
    /// position.
    ///
    /// # Parameters
    /// - `reader`: Seekable input whose current position marks the content
    ///   window start.
    ///
    /// # Returns
    /// MIME candidates returned by the wrapped backend.
    ///
    /// # Errors
    /// Propagates backend and I/O errors. A reader-position restoration error
    /// takes precedence over a backend error. Prefix requirements larger
    /// than the configured reader budget return
    /// [`crate::MimeError::BufferLimitExceeded`].
    fn detect_reader(&self, reader: &mut dyn qubit_io::std_io::ReadSeek) -> MimeResult<Vec<String>> {
        use std::io::SeekFrom;

        let position = reader.stream_position()?;
        let requirement = self.content_requirement();
        let budget = self.max_reader_buffer_size();
        let result = (|| {
            if let ContentRequirement::Prefix(limit) = requirement
                && limit > budget
            {
                return Err(crate::MimeError::BufferLimitExceeded {
                    requested: limit,
                    limit: budget,
                });
            }
            let mut view = RemainingContentReader::new(reader)?;
            self.backend
                .guess_from_reader(&mut view)
                .map(|(candidates, _content)| candidates)
        })();
        let restore = reader.seek(SeekFrom::Start(position));
        match (result, restore) {
            (Ok(candidates), Ok(_)) => Ok(candidates),
            (Err(error), Ok(_)) => Err(error),
            (_, Err(error)) => Err(crate::MimeError::Io(error)),
        }
    }
}
