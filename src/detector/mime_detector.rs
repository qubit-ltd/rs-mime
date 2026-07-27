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

use qubit_fs::{
    FileResource,
    ReadOptions,
};
use qubit_io::{
    Input,
    ReadSeek,
};

use crate::{
    MimeDetectionPolicy,
    MimeResult,
};

/// Detects MIME types from filenames and content.
pub trait MimeDetector: Debug + Send + Sync {
    /// Detects a MIME type from a filename.
    ///
    /// # Parameters
    /// - `filename`: File path or basename.
    ///
    /// # Returns
    /// First matching MIME type name, or `None`.
    fn detect_by_filename(&self, filename: &str) -> Option<String>;

    /// Detects a MIME type from content bytes.
    ///
    /// # Parameters
    /// - `content`: Content bytes to inspect.
    ///
    /// # Returns
    /// First matching MIME type name, or `None`.
    fn detect_by_content(&self, content: &[u8]) -> Option<String>;

    /// Detects a MIME type from content bytes and an optional filename.
    ///
    /// # Parameters
    /// - `content`: Content bytes to inspect.
    /// - `filename`: Optional file path or basename.
    /// - `policy`: Strategy for resolving filename and content results.
    ///
    /// # Returns
    /// Selected MIME type name, or `None`.
    fn detect(
        &self,
        content: &[u8],
        filename: Option<&str>,
        policy: MimeDetectionPolicy,
    ) -> Option<String>;

    /// Detects a MIME type from a seekable reader without consuming its
    /// position.
    ///
    /// # Parameters
    /// - `reader`: Reader to inspect. The original stream position is restored.
    /// - `filename`: Optional path or basename used for filename detection.
    /// - `policy`: Strategy for resolving filename and content results.
    ///
    /// # Returns
    /// Selected MIME type name, or `None`.
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

    /// Detects a MIME type through a provider-neutral filesystem resource.
    ///
    /// # Errors
    /// Returns a filesystem or detector error when the resource cannot be
    /// opened or inspected.
    ///
    /// `max_bytes` bounds the prefix read used for content inspection; larger
    /// resources remain eligible for detection.
    fn detect_resource(
        &self,
        resource: &FileResource,
        max_bytes: usize,
        policy: MimeDetectionPolicy,
    ) -> MimeResult<Option<String>> {
        let mut reader = resource.open_reader(ReadOptions::default())?;
        let mut content = vec![0; max_bytes];
        let read = reader.read_fully(&mut content)?;
        content.truncate(read);
        let filename = resource
            .path()
            .as_str()
            .rsplit('/')
            .next()
            .filter(|name| !name.is_empty());
        Ok(self.detect(&content, filename, policy))
    }
}

impl MimeDetector for Box<dyn MimeDetector> {
    /// Delegates filename detection to the boxed detector.
    fn detect_by_filename(&self, filename: &str) -> Option<String> {
        self.as_ref().detect_by_filename(filename)
    }

    /// Delegates content detection to the boxed detector.
    fn detect_by_content(&self, content: &[u8]) -> Option<String> {
        self.as_ref().detect_by_content(content)
    }

    /// Delegates combined detection to the boxed detector.
    fn detect(
        &self,
        content: &[u8],
        filename: Option<&str>,
        policy: MimeDetectionPolicy,
    ) -> Option<String> {
        self.as_ref().detect(content, filename, policy)
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

    fn detect_resource(
        &self,
        resource: &FileResource,
        max_bytes: usize,
        policy: MimeDetectionPolicy,
    ) -> MimeResult<Option<String>> {
        self.as_ref().detect_resource(resource, max_bytes, policy)
    }
}

impl MimeDetector for Arc<dyn MimeDetector> {
    /// Delegates filename detection to the shared detector.
    fn detect_by_filename(&self, filename: &str) -> Option<String> {
        self.as_ref().detect_by_filename(filename)
    }

    /// Delegates content detection to the shared detector.
    fn detect_by_content(&self, content: &[u8]) -> Option<String> {
        self.as_ref().detect_by_content(content)
    }

    /// Delegates combined detection to the shared detector.
    fn detect(
        &self,
        content: &[u8],
        filename: Option<&str>,
        policy: MimeDetectionPolicy,
    ) -> Option<String> {
        self.as_ref().detect(content, filename, policy)
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

    fn detect_resource(
        &self,
        resource: &FileResource,
        max_bytes: usize,
        policy: MimeDetectionPolicy,
    ) -> MimeResult<Option<String>> {
        self.as_ref().detect_resource(resource, max_bytes, policy)
    }
}
