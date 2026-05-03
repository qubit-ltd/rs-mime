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
