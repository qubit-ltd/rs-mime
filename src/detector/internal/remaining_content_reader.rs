// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Borrowed seekable view whose offset zero is the reader's current position.

use std::io::Error;
use std::io::ErrorKind;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;

use qubit_io::std_io::ReadSeek;

/// Provides a seekable view over content remaining from a reader's current
/// position.
pub(crate) struct RemainingContentReader<'a> {
    /// Underlying seekable reader borrowed for the lifetime of this view.
    reader: &'a mut dyn ReadSeek,
    /// Absolute position corresponding to logical offset zero.
    base: u64,
}

impl<'a> RemainingContentReader<'a> {
    /// Creates a view starting at the reader's current position.
    ///
    /// # Parameters
    /// - `reader`: Underlying seekable reader.
    ///
    /// # Returns
    /// A view whose logical start is the reader's current position.
    ///
    /// # Errors
    /// Returns an I/O error when the current reader position cannot be
    /// obtained.
    pub(crate) fn new(reader: &'a mut dyn ReadSeek) -> std::io::Result<Self> {
        let base = reader.stream_position()?;
        Ok(Self { reader, base })
    }
}

impl Read for RemainingContentReader<'_> {
    /// Reads bytes from the underlying reader at the current logical position.
    ///
    /// # Parameters
    /// - `buffer`: Destination for read bytes.
    ///
    /// # Returns
    /// The number of bytes read.
    ///
    /// # Errors
    /// Propagates read errors from the underlying reader.
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.reader.read(buffer)
    }
}

impl Seek for RemainingContentReader<'_> {
    /// Moves the logical reader position while keeping it at or after the view
    /// start.
    ///
    /// # Parameters
    /// - `from`: Position relative to the view start, current position, or EOF.
    ///
    /// # Returns
    /// The new logical position relative to the view start.
    ///
    /// # Errors
    /// Returns an I/O error for a failed underlying seek, an offset overflow,
    /// or a target before the view start.
    fn seek(&mut self, from: SeekFrom) -> std::io::Result<u64> {
        let absolute = match from {
            SeekFrom::Start(offset) => self.base.checked_add(offset).ok_or_else(invalid_seek)?,
            SeekFrom::Current(offset) => {
                let current = self.reader.stream_position()?;
                checked_offset(current, offset)?
            }
            SeekFrom::End(offset) => {
                let current = self.reader.stream_position()?;
                let end = self.reader.seek(SeekFrom::End(0))?;
                match checked_offset(end, offset) {
                    Ok(absolute) if absolute >= self.base => absolute,
                    Ok(_) => {
                        self.reader.seek(SeekFrom::Start(current))?;
                        return Err(invalid_seek());
                    }
                    Err(error) => {
                        self.reader.seek(SeekFrom::Start(current))?;
                        return Err(error);
                    }
                }
            }
        };
        let logical = absolute.checked_sub(self.base).ok_or_else(invalid_seek)?;
        self.reader.seek(SeekFrom::Start(absolute))?;
        Ok(logical)
    }
}

/// Applies a signed offset to an absolute position with overflow checks.
///
/// # Parameters
/// - `base`: Absolute position to adjust.
/// - `offset`: Signed byte offset.
///
/// # Returns
/// The adjusted absolute position.
///
/// # Errors
/// Returns [`std::io::ErrorKind::InvalidInput`] when the adjustment overflows.
fn checked_offset(base: u64, offset: i64) -> std::io::Result<u64> {
    let result = if offset >= 0 {
        base.checked_add(offset as u64)
    } else {
        base.checked_sub(offset.unsigned_abs())
    };
    result.ok_or_else(invalid_seek)
}

/// Creates an error for an invalid logical seek target.
///
/// # Returns
/// An invalid-input I/O error.
fn invalid_seek() -> Error {
    Error::new(ErrorKind::InvalidInput, "seek is outside the remaining content window")
}
