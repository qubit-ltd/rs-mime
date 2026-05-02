/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Backend contract used by string-list MIME detectors.

/// Backend contract used by string-list MIME detectors.
pub trait StringListMimeDetectorBackend {
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
    fn guess_from_content(&self, content: &[u8]) -> Vec<String>;
}
