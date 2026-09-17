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

/// MIME detector that keeps the repository alive through an `Arc` handle.
#[derive(Debug)]
pub(crate) struct OwnedRepositoryMimeDetector {
    core: MimeDetectorCore,
    repository: Arc<MimeRepository>,
}

impl OwnedRepositoryMimeDetector {
    /// Creates an owned repository detector.
    pub(crate) fn new(config: MimeConfig, repository: Arc<MimeRepository>) -> Self {
        Self {
            core: MimeDetectorCore::from_mime_config(config),
            repository,
        }
    }
}

impl StreamBasedMimeDetector for OwnedRepositoryMimeDetector {
    fn core(&self) -> &MimeDetectorCore {
        &self.core
    }

    fn max_test_bytes(&self) -> usize {
        self.repository.max_test_bytes()
    }

    fn guess_from_filename(&self, filename: &str) -> Vec<String> {
        self.repository
            .detect_by_filename(filename)
            .into_iter()
            .map(|mime| mime.name().to_owned())
            .collect()
    }

    fn guess_from_content_bytes(&self, content: &[u8]) -> MimeResult<Vec<String>> {
        Ok(self
            .repository
            .detect_by_content(content)
            .into_iter()
            .map(|mime| mime.name().to_owned())
            .collect())
    }
}
