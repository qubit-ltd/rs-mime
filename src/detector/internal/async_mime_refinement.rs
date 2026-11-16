// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Managed asynchronous media refinement helpers.

use crate::MediaStreamType;
use crate::MimeError;
use crate::command_execution::MimeCommandOutput;
use crate::command_execution::require_complete_stdout;

/// Converts a completed ffprobe invocation into the domain result.
///
/// # Parameters
/// - `output`: Completed command result, including its exit code and stdout.
///
/// # Returns
/// The classified stream type, or a backend error when stdout is incomplete or
/// ffprobe did not produce an exit code.
pub(crate) fn stream_type_from_output(output: &MimeCommandOutput) -> Result<MediaStreamType, MimeError> {
    match output.exit_code {
        Some(0) => {
            let stdout = require_complete_stdout(output).map_err(|reason| MimeError::ClassifierBackend {
                backend: "ffprobe".to_owned(),
                reason: reason.to_owned(),
            })?;
            Ok(crate::FfprobeCommandMediaStreamClassifier::classify_stream_listing(
                stdout,
            ))
        }
        Some(_) => Ok(MediaStreamType::None),
        None => Err(MimeError::ClassifierBackend {
            backend: "ffprobe".to_owned(),
            reason: "ffprobe exited without a numeric exit code".to_owned(),
        }),
    }
}
