// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Top-level MIME detector interface.

use std::fmt::Debug;
use std::path::Path;
use std::sync::Arc;

use qubit_fs::FileSystem;
use qubit_fs::Path as FsPath;
use qubit_fs::ReadOptions;
use qubit_io::std_io::ReadSeek;

use crate::MimeDetectionPolicy;
use crate::MimeError;
use crate::MimeResult;

/// Detects MIME types from filenames and content.
pub trait MimeDetector: Debug + Send + Sync {
    /// Detects a MIME type from a filename.
    ///
    /// # Parameters
    /// - `filename`: File path or basename.
    ///
    /// # Returns
    /// `Ok(Some(_))` contains the first matching MIME type; `Ok(None)` means
    /// no filename rule matched.
    fn detect_by_filename(&self, filename: &str) -> MimeResult<Option<String>>;

    /// Detects a MIME type from content bytes.
    ///
    /// # Parameters
    /// - `content`: Content bytes to inspect.
    ///
    /// # Returns
    /// `Ok(Some(_))` contains the first matching MIME type; `Ok(None)` means
    /// no content rule matched.
    ///
    /// # Errors
    /// Returns a backend or media-classifier error when content inspection or
    /// refinement cannot complete.
    fn detect_by_content(&self, content: &[u8]) -> MimeResult<Option<String>>;

    /// Detects a MIME type from content bytes and an optional filename.
    ///
    /// # Parameters
    /// - `content`: Content bytes to inspect.
    /// - `filename`: Optional file path or basename.
    /// - `policy`: Strategy for resolving filename and content results.
    ///
    /// # Returns
    /// `Ok(Some(_))` contains the selected MIME type; `Ok(None)` means neither
    /// filename nor content produced a candidate.
    fn detect(
        &self,
        content: &[u8],
        filename: Option<&str>,
        policy: MimeDetectionPolicy,
    ) -> MimeResult<Option<String>>;

    /// Gets the largest prefix buffer this detector may allocate.
    ///
    /// # Returns
    /// The inclusive byte limit enforced before reader and path prefix reads.
    fn max_buffer_size(&self) -> usize;

    /// Detects a MIME type from a seekable reader without consuming its
    /// position.
    ///
    /// # Parameters
    /// - `reader`: Reader to inspect. The original stream position is restored.
    /// - `filename`: Optional path or basename used for filename detection.
    /// - `policy`: Strategy for resolving filename and content results.
    ///
    /// # Returns
    /// `Ok(Some(_))` contains the selected MIME type; `Ok(None)` means no
    /// candidate matched.
    ///
    /// # Errors
    /// Returns [`MimeError::Io`](crate::MimeError::Io) when reading or seeking
    /// fails.
    fn detect_reader(
        &self,
        reader: &mut dyn ReadSeek,
        filename: Option<&str>,
        policy: MimeDetectionPolicy,
    ) -> MimeResult<Option<String>>;

    /// Detects a MIME type from a local file.
    ///
    /// # Parameters
    /// - `file`: Local file path.
    /// - `policy`: Strategy for resolving filename and content results.
    ///
    /// # Returns
    /// Selected MIME type name, or `None`.
    ///
    /// # Errors
    /// Returns [`MimeError::Io`](crate::MimeError::Io) when the file cannot be
    /// opened or read, or another [`MimeError`](crate::MimeError) when a
    /// detector backend fails.
    fn detect_file(
        &self,
        file: &Path,
        policy: MimeDetectionPolicy,
    ) -> MimeResult<Option<String>>;

    /// Detects a MIME type through a provider-neutral filesystem path.
    ///
    /// # Parameters
    /// - `file_system`: Filesystem facade that owns `path`.
    /// - `path`: Provider-neutral path to inspect.
    /// - `max_bytes`: Maximum prefix length read for content inspection.
    /// - `policy`: Strategy for resolving filename and content results.
    ///
    /// # Returns
    /// Selected MIME type name, or `None`.
    ///
    /// # Errors
    /// Returns [`MimeError::BufferLimitExceeded`] when `max_bytes` exceeds the
    /// detector limit, or a filesystem/detector error when `path` cannot be
    /// inspected.
    ///
    /// `max_bytes` bounds the prefix read used for content inspection; larger
    /// resources remain eligible for detection.
    fn detect_path(
        &self,
        file_system: &FileSystem,
        path: &FsPath,
        max_bytes: usize,
        policy: MimeDetectionPolicy,
    ) -> MimeResult<Option<String>> {
        let limit = self.max_buffer_size();
        if max_bytes > limit {
            return Err(MimeError::BufferLimitExceeded {
                requested: max_bytes,
                limit,
            });
        }
        let content =
            file_system.read_prefix(path, ReadOptions::default(), max_bytes)?;
        let filename = path.file_name();
        self.detect(&content, filename, policy)
    }
}

impl MimeDetector for Box<dyn MimeDetector> {
    /// Delegates filename detection to the boxed detector.
    fn detect_by_filename(&self, filename: &str) -> MimeResult<Option<String>> {
        self.as_ref().detect_by_filename(filename)
    }

    /// Delegates content detection to the boxed detector.
    fn detect_by_content(&self, content: &[u8]) -> MimeResult<Option<String>> {
        self.as_ref().detect_by_content(content)
    }

    /// Delegates combined detection to the boxed detector.
    fn detect(
        &self,
        content: &[u8],
        filename: Option<&str>,
        policy: MimeDetectionPolicy,
    ) -> MimeResult<Option<String>> {
        self.as_ref().detect(content, filename, policy)
    }

    fn max_buffer_size(&self) -> usize {
        self.as_ref().max_buffer_size()
    }

    /// Delegates reader detection to the boxed detector.
    fn detect_reader(
        &self,
        reader: &mut dyn ReadSeek,
        filename: Option<&str>,
        policy: MimeDetectionPolicy,
    ) -> MimeResult<Option<String>> {
        self.as_ref().detect_reader(reader, filename, policy)
    }

    /// Delegates file detection to the boxed detector.
    fn detect_file(
        &self,
        file: &Path,
        policy: MimeDetectionPolicy,
    ) -> MimeResult<Option<String>> {
        self.as_ref().detect_file(file, policy)
    }

    fn detect_path(
        &self,
        file_system: &FileSystem,
        path: &FsPath,
        max_bytes: usize,
        policy: MimeDetectionPolicy,
    ) -> MimeResult<Option<String>> {
        self.as_ref()
            .detect_path(file_system, path, max_bytes, policy)
    }
}

impl MimeDetector for Arc<dyn MimeDetector> {
    /// Delegates filename detection to the shared detector.
    fn detect_by_filename(&self, filename: &str) -> MimeResult<Option<String>> {
        self.as_ref().detect_by_filename(filename)
    }

    /// Delegates content detection to the shared detector.
    fn detect_by_content(&self, content: &[u8]) -> MimeResult<Option<String>> {
        self.as_ref().detect_by_content(content)
    }

    /// Delegates combined detection to the shared detector.
    fn detect(
        &self,
        content: &[u8],
        filename: Option<&str>,
        policy: MimeDetectionPolicy,
    ) -> MimeResult<Option<String>> {
        self.as_ref().detect(content, filename, policy)
    }

    fn max_buffer_size(&self) -> usize {
        self.as_ref().max_buffer_size()
    }

    /// Delegates reader detection to the shared detector.
    fn detect_reader(
        &self,
        reader: &mut dyn ReadSeek,
        filename: Option<&str>,
        policy: MimeDetectionPolicy,
    ) -> MimeResult<Option<String>> {
        self.as_ref().detect_reader(reader, filename, policy)
    }

    /// Delegates file detection to the shared detector.
    fn detect_file(
        &self,
        file: &Path,
        policy: MimeDetectionPolicy,
    ) -> MimeResult<Option<String>> {
        self.as_ref().detect_file(file, policy)
    }

    fn detect_path(
        &self,
        file_system: &FileSystem,
        path: &FsPath,
        max_bytes: usize,
        policy: MimeDetectionPolicy,
    ) -> MimeResult<Option<String>> {
        self.as_ref()
            .detect_path(file_system, path, max_bytes, policy)
    }
}
