/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Provider for the built-in repository-backed MIME detector.

use crate::{
    MimeConfig,
    MimeDetector,
    MimeResult,
    RepositoryMimeDetector,
};

use super::MimeDetectorProvider;

/// Provider for the built-in repository-backed detector.
#[derive(Debug, Clone, Copy, Default)]
pub struct RepositoryMimeDetectorProvider;

impl MimeDetectorProvider for RepositoryMimeDetectorProvider {
    /// Gets the canonical provider identifier.
    fn id(&self) -> &'static str {
        "repository"
    }

    /// Gets repository detector aliases.
    fn aliases(&self) -> &'static [&'static str] {
        &["repository-mime-detector"]
    }

    /// Creates a repository-backed detector.
    fn create(&self, config: &MimeConfig) -> MimeResult<Box<dyn MimeDetector>> {
        Ok(Box::new(RepositoryMimeDetector::from_mime_config(
            config.clone(),
        )))
    }
}
