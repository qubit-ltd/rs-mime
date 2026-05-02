/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! MIME detector kind selector and helpers.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MimeDetectorKind {
    Repository,
    FileCommand,
}

impl MimeDetectorKind {
    /// Selects a detector kind from configuration and backend availability.
    ///
    /// # Parameters
    /// - `configured`: Configured detector selector.
    /// - `file_available`: Whether the `file` backend is available.
    ///
    /// # Returns
    /// Selected detector kind.
    pub(crate) fn select(configured: &str, file_available: bool) -> Self {
        if let Some(backend) = Self::from_name(configured) {
            backend
        } else if file_available {
            Self::FileCommand
        } else {
            Self::Repository
        }
    }

    /// Resolves a detector kind from a configured implementation name.
    ///
    /// # Parameters
    /// - `name`: Implementation selector.
    ///
    /// # Returns
    /// Matching backend, or `None` when the selector is empty or unknown.
    pub(crate) fn from_name(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "repository" | "repository-mime-detector" => Some(Self::Repository),
            "file" | "file-command" | "file-command-mime-detector" => Some(Self::FileCommand),
            _ => None,
        }
    }
}
