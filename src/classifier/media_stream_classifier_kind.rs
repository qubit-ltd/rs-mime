/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Media stream classifier backend selector and helpers.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MediaStreamClassifierKind {
    FfprobeCommand,
}

impl MediaStreamClassifierKind {
    /// Selects a classifier backend from configuration.
    ///
    /// # Parameters
    /// - `configured`: Configured classifier selector.
    ///
    /// # Returns
    /// Selected classifier backend.
    pub(crate) fn select(configured: &str) -> Self {
        if let Some(backend) = Self::from_name(configured) {
            backend
        } else {
            Self::FfprobeCommand
        }
    }

    /// Resolves a classifier backend from a configured implementation name.
    ///
    /// # Parameters
    /// - `name`: Implementation selector.
    ///
    /// # Returns
    /// Matching backend, or `None` when the selector is empty or unknown.
    pub(crate) fn from_name(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "ffprobe" | "ffprobe-command" | "ffprobe-command-media-stream-classifier" => {
                Some(Self::FfprobeCommand)
            }
            _ => None,
        }
    }
}
