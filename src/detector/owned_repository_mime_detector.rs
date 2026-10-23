// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Repository MIME detector with shared repository ownership.

use std::sync::Arc;

use super::mime_detector_core::MimeDetectorCore;
use super::stream_based_mime_detector::StreamBasedMimeDetector;
use crate::MimeConfig;
use crate::MimeRepository;
use crate::MimeResult;

/// MIME detector that shares a repository without copying its detection data.
///
/// The shared handle keeps repository rules available for the detector's
/// lifetime, including filename matching, content matching, and read sizing.
#[derive(Debug)]
pub(crate) struct OwnedRepositoryMimeDetector {
    core: MimeDetectorCore,
    repository: Arc<MimeRepository>,
}

impl OwnedRepositoryMimeDetector {
    /// Creates a detector using the supplied configuration and shared
    /// repository.
    ///
    /// # Parameters
    /// - `config`: Settings for candidate selection and content inspection.
    /// - `repository`: Shared filename and content detection rules.
    ///
    /// # Returns
    /// A detector that retains a shared reference to `repository`.
    #[must_use]
    pub(crate) fn new(config: MimeConfig, repository: Arc<MimeRepository>) -> Self {
        Self {
            core: MimeDetectorCore::from_mime_config(config),
            repository,
        }
    }
}

impl StreamBasedMimeDetector for OwnedRepositoryMimeDetector {
    /// Returns configuration and candidate-selection behavior shared by
    /// detectors.
    #[inline]
    fn core(&self) -> &MimeDetectorCore {
        &self.core
    }

    /// Returns the largest content prefix needed by any repository rule.
    #[inline]
    fn max_test_bytes(&self) -> usize {
        self.repository.max_test_bytes()
    }

    /// Returns repository candidate names matched against a filename.
    ///
    /// # Parameters
    /// - `filename`: Path or basename whose extension is matched.
    ///
    /// # Returns
    /// Candidate MIME names in repository detection order.
    fn guess_from_filename(&self, filename: &str) -> Vec<String> {
        self.repository
            .detect_by_filename(filename)
            .into_iter()
            .map(|mime| mime.name().to_owned())
            .collect()
    }

    /// Returns repository candidate names matched against content bytes.
    ///
    /// # Parameters
    /// - `content`: Bytes supplied to repository content detection.
    ///
    /// # Returns
    /// Candidate MIME names in repository detection order.
    ///
    /// # Errors
    /// Returns an error if repository content detection fails.
    fn guess_from_content_bytes(&self, content: &[u8]) -> MimeResult<Vec<String>> {
        Ok(self
            .repository
            .detect_by_content(content)
            .into_iter()
            .map(|mime| mime.name().to_owned())
            .collect())
    }
}
