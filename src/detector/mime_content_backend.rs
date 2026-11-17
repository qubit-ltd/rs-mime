// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Object-safe backend contract for MIME content providers.

use std::fmt::Debug;
use std::io::SeekFrom;

use qubit_io::std_io::ReadSeek;

pub use super::content_requirement::ContentRequirement;
pub use super::mime_detector_adapter::MimeDetectorAdapter;
use crate::MimeError;
use crate::MimeResult;

/// Object-safe contract for content-based MIME providers.
///
/// # Examples
///
/// ```
/// use qubit_mime::ContentRequirement;
/// use qubit_mime::MimeContentBackend;
/// use qubit_mime::MimeResult;
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
    ///
    /// # Returns
    /// The required prefix length, or `Complete` when all available bytes are
    /// needed.
    fn content_requirement(&self) -> ContentRequirement;

    /// Returns MIME candidates identified from the supplied content bytes.
    ///
    /// # Parameters
    /// - `bytes`: Content bytes available for MIME detection.
    ///
    /// # Returns
    /// MIME type names identified by this backend, ordered by preference.
    ///
    /// # Errors
    /// Returns a backend error when the supplied bytes cannot be inspected.
    fn detect_bytes(&self, bytes: &[u8]) -> MimeResult<Vec<String>>;

    /// Returns the maximum number of bytes buffered by the default reader
    /// implementation.
    ///
    /// This budget applies only to the default `detect_reader` implementation.
    /// An override may use a different bounded strategy, such as sampling
    /// through random access.
    ///
    /// # Returns
    /// The reader buffer budget, which defaults to 16 MiB.
    fn max_reader_buffer_size(&self) -> usize {
        crate::DEFAULT_MIME_MAX_BUFFER_SIZE
    }

    /// Reads the required content from a seekable reader and detects MIME
    /// candidates from those bytes.
    ///
    /// The reader is returned to its original position before detection. When
    /// reading and restoring both fail, the restore error takes precedence.
    /// The byte budget applies to this default implementation; custom
    /// `detect_reader` overrides define their own resource behavior.
    ///
    /// # Parameters
    /// - `reader`: Seekable input whose position is restored after reading.
    ///
    /// # Returns
    /// MIME type names identified from the required input bytes.
    ///
    /// # Errors
    /// Returns an I/O error when reading or restoring the reader position
    /// fails, [`MimeError::BufferLimitExceeded`] when the reader request
    /// exceeds its buffer budget, [`MimeError::BufferAllocationFailed`]
    /// when buffering cannot reserve memory, or a backend error when
    /// detection fails.
    fn detect_reader(&self, reader: &mut dyn ReadSeek) -> MimeResult<Vec<String>> {
        let position = reader.stream_position()?;
        let read_result = read_required(reader, self.content_requirement(), self.max_reader_buffer_size());
        let restore_result = reader.seek(SeekFrom::Start(position));
        match (read_result, restore_result) {
            (Ok(bytes), Ok(_)) => self.detect_bytes(&bytes),
            (Err(error), Ok(_)) => Err(error),
            (_, Err(error)) => Err(MimeError::Io(error)),
        }
    }
}

/// Retries a read when the reader reports a temporary interruption.
///
/// # Parameters
/// - `reader`: Seekable input to read from.
/// - `buffer`: Destination for the bytes read.
fn read_retry_interrupted(reader: &mut dyn ReadSeek, buffer: &mut [u8]) -> std::io::Result<usize> {
    loop {
        match reader.read(buffer) {
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            result => return result,
        }
    }
}

/// Reads the required prefix or complete content within the supplied buffer
/// budget.
///
/// # Parameters
/// - `reader`: Seekable input to read from its current position.
/// - `requirement`: Prefix length or complete-content requirement.
/// - `budget`: Maximum number of bytes to buffer.
///
/// # Returns
/// Bytes read from the requested content window.
///
/// # Errors
/// Returns an I/O error on read failure, [`MimeError::BufferLimitExceeded`]
/// when the request exceeds the budget, or
/// [`MimeError::BufferAllocationFailed`] when allocation fails. Complete reads
/// probe one extra byte to detect input exceeding the budget.
fn read_required(reader: &mut dyn ReadSeek, requirement: ContentRequirement, budget: usize) -> MimeResult<Vec<u8>> {
    if let ContentRequirement::Prefix(limit) = requirement
        && limit > budget
    {
        return Err(MimeError::BufferLimitExceeded {
            requested: limit,
            limit: budget,
        });
    }
    let mut bytes = Vec::new();
    let target = match requirement {
        ContentRequirement::Prefix(limit) => limit,
        ContentRequirement::Complete => budget,
    };
    let mut chunk = [0_u8; 8192];
    while bytes.len() < target {
        let chunk_size = chunk.len().min(target - bytes.len());
        let count = read_retry_interrupted(reader, &mut chunk[..chunk_size])?;
        if count == 0 {
            return Ok(bytes);
        }
        bytes
            .try_reserve(count)
            .map_err(|source| MimeError::BufferAllocationFailed {
                requested: bytes.len().saturating_add(count),
                source,
            })?;
        bytes.extend_from_slice(&chunk[..count]);
    }
    if requirement == ContentRequirement::Complete {
        let mut probe = [0_u8; 1];
        if read_retry_interrupted(reader, &mut probe)? != 0 {
            return Err(MimeError::BufferLimitExceeded {
                requested: budget.saturating_add(1),
                limit: budget,
            });
        }
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::ContentRequirement;
    use super::MimeContentBackend;
    use crate::MimeResult;

    #[derive(Debug)]
    struct PrefixBackend;

    impl MimeContentBackend for PrefixBackend {
        fn content_requirement(&self) -> ContentRequirement {
            ContentRequirement::Prefix(3)
        }

        fn detect_bytes(&self, bytes: &[u8]) -> MimeResult<Vec<String>> {
            Ok(vec![String::from_utf8_lossy(bytes).into_owned()])
        }
    }

    #[test]
    fn test_default_reader_buffers_declared_prefix_and_restores_position() {
        let backend = PrefixBackend;
        let mut reader = Cursor::new(b"_abcdef".to_vec());
        reader.set_position(1);

        assert_eq!(backend.max_reader_buffer_size(), crate::DEFAULT_MIME_MAX_BUFFER_SIZE);
        assert_eq!(backend.detect_reader(&mut reader).unwrap(), ["abc"]);
        assert_eq!(reader.position(), 1);
    }

    #[test]
    fn test_default_reader_rejects_complete_content_over_budget_and_restores() {
        #[derive(Debug)]
        struct CompleteBackend;

        impl MimeContentBackend for CompleteBackend {
            fn content_requirement(&self) -> ContentRequirement {
                ContentRequirement::Complete
            }

            fn max_reader_buffer_size(&self) -> usize {
                3
            }

            fn detect_bytes(&self, bytes: &[u8]) -> MimeResult<Vec<String>> {
                Ok(vec![bytes.len().to_string()])
            }
        }

        let mut reader = Cursor::new(b"_abcd".to_vec());
        reader.set_position(1);
        let error = CompleteBackend
            .detect_reader(&mut reader)
            .expect_err("complete input one byte above the limit must fail");

        assert!(matches!(
            error,
            crate::MimeError::BufferLimitExceeded { requested: 4, limit: 3 }
        ));
        assert_eq!(reader.position(), 1);

        let mut within_limit = Cursor::new(b"xy".to_vec());
        assert_eq!(CompleteBackend.detect_reader(&mut within_limit).unwrap(), ["2"]);
        assert_eq!(within_limit.position(), 0);
    }
}
