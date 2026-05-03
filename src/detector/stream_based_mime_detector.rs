/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Helpers for stream-backed MIME detectors.

use std::io::SeekFrom;

use qubit_io::ReadSeek;

use crate::MimeResult;

/// Reads a prefix from a stream and restores the original position.
///
/// # Parameters
/// - `reader`: Stream to inspect.
/// - `max_bytes`: Maximum number of bytes to read.
///
/// # Returns
/// Bytes read from the stream.
///
/// # Errors
/// Returns [`MimeError::Io`](crate::MimeError::Io) when reading or seeking fails.
pub(crate) fn read_prefix(reader: &mut dyn ReadSeek, max_bytes: usize) -> MimeResult<Vec<u8>> {
    let position = reader.stream_position()?;
    let mut buffer = vec![0; max_bytes];
    let bytes_read = reader.read(&mut buffer)?;
    buffer.truncate(bytes_read);
    reader.seek(SeekFrom::Start(position))?;
    Ok(buffer)
}

// qubit-style: allow coverage-cfg
#[cfg(coverage)]
pub(crate) mod coverage_support {
    //! Coverage helpers for stream-based detector helpers.

    use std::io::{Cursor, Error, ErrorKind, Read, Result as IoResult, Seek, SeekFrom};

    use super::read_prefix;

    /// Exercises successful and failing stream prefix reads.
    ///
    /// # Returns
    /// Summary strings from stream reads.
    pub(crate) fn exercise_stream_edges() -> Vec<String> {
        let mut cursor = Cursor::new(b"abcdef".to_vec());
        let prefix = read_prefix(&mut cursor, 3).expect("prefix should read");
        let mut failing = FailingSeek;
        let error = read_prefix(&mut failing, 3)
            .expect_err("seek should fail")
            .to_string();
        let mut readable_failing_seek = FailingSeek;
        let read_from_failing_seek = Read::read(&mut readable_failing_seek, &mut [])
            .expect("empty read should succeed")
            .to_string();
        let mut failing_read = FailingRead;
        let read_error = read_prefix(&mut failing_read, 3)
            .expect_err("read should fail")
            .to_string();
        let mut seekable_failing_read = FailingRead;
        let seek_from_failing_read = Seek::seek(&mut seekable_failing_read, SeekFrom::Start(0))
            .expect("seek should succeed")
            .to_string();
        vec![
            String::from_utf8_lossy(&prefix).into_owned(),
            error,
            read_from_failing_seek,
            read_error,
            seek_from_failing_read,
        ]
    }

    struct FailingSeek;

    impl Read for FailingSeek {
        /// Returns no bytes.
        fn read(&mut self, _buf: &mut [u8]) -> IoResult<usize> {
            Ok(0)
        }
    }

    impl Seek for FailingSeek {
        /// Always fails seeking.
        fn seek(&mut self, _pos: SeekFrom) -> IoResult<u64> {
            Err(Error::new(ErrorKind::Other, "seek failed"))
        }
    }

    struct FailingRead;

    impl Read for FailingRead {
        /// Always fails reading.
        fn read(&mut self, _buf: &mut [u8]) -> IoResult<usize> {
            Err(Error::new(ErrorKind::Other, "read failed"))
        }
    }

    impl Seek for FailingRead {
        /// Returns a successful stream position.
        fn seek(&mut self, _pos: SeekFrom) -> IoResult<u64> {
            Ok(0)
        }
    }
}
