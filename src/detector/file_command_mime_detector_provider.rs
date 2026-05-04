/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Provider for the built-in system `file` command MIME detector.

use crate::{
    FileCommandMimeDetector,
    MimeConfig,
    MimeDetector,
    MimeResult,
};

use super::{
    MimeDetectorAvailability,
    MimeDetectorProvider,
};

/// Provider for the built-in system `file` command detector.
#[derive(Debug, Clone, Copy, Default)]
pub struct FileCommandMimeDetectorProvider;

impl MimeDetectorProvider for FileCommandMimeDetectorProvider {
    /// Gets the canonical provider identifier.
    fn id(&self) -> &'static str {
        "file"
    }

    /// Gets file command detector aliases.
    fn aliases(&self) -> &'static [&'static str] {
        &["file-command", "file-command-mime-detector"]
    }

    /// Gives the file command provider higher auto priority than repository.
    fn priority(&self) -> i32 {
        10
    }

    /// Checks whether the `file` command is available.
    fn availability(&self, _config: &MimeConfig) -> MimeDetectorAvailability {
        if FileCommandMimeDetector::is_available() {
            MimeDetectorAvailability::Available
        } else {
            MimeDetectorAvailability::Unavailable {
                reason: "`file` command is not available".to_owned(),
            }
        }
    }

    /// Creates a file-command-backed detector.
    fn create(&self, config: &MimeConfig) -> MimeResult<Box<dyn MimeDetector>> {
        Ok(Box::new(FileCommandMimeDetector::from_mime_config(
            config.clone(),
        )))
    }
}
