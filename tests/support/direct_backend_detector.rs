// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_mime::MimeDetectorBackend;
use qubit_mime::MimeDetectorCore;
use qubit_mime::MimeResult;

/// Detector fixture exercising `MimeDetectorBackend` default entry points.
#[derive(Debug)]
pub(crate) struct DirectBackendDetector {
    /// Shared detection coordination used by backend defaults.
    core: MimeDetectorCore,
}

impl DirectBackendDetector {
    /// Creates a detector with the default coordination core.
    ///
    /// # Returns
    ///
    /// A backend fixture recognizing `hello` and `.txt` inputs.
    #[inline]
    pub(crate) fn new() -> Self {
        Self {
            core: MimeDetectorCore::default(),
        }
    }
}

impl MimeDetectorBackend for DirectBackendDetector {
    fn core(&self) -> &MimeDetectorCore {
        &self.core
    }

    fn max_test_bytes(&self) -> usize {
        5
    }

    fn guess_from_filename(&self, filename: &str) -> Vec<String> {
        if filename.ends_with(".txt") {
            vec!["text/plain".to_owned()]
        } else {
            Vec::new()
        }
    }

    fn guess_from_content(&self, content: &[u8]) -> MimeResult<Vec<String>> {
        if content == b"hello" {
            Ok(vec!["text/plain".to_owned()])
        } else {
            Ok(Vec::new())
        }
    }
}
