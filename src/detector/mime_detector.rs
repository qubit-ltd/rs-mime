/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Top-level MIME detector interface.

/// Detects MIME types from filenames and content.
pub trait MimeDetector {
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
        policy: crate::MimeDetectionPolicy,
    ) -> Option<String>;
}
