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
///
/// # Errors
/// Returns [`MimeError::ClassifierBackend`] when stdout is incomplete after a
/// successful exit or ffprobe provides no numeric exit code.
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

#[cfg(test)]
mod tests {
    use super::stream_type_from_output;
    use crate::MediaStreamType;
    use crate::MimeError;
    use crate::command_execution::MimeCommandOutput;

    #[test]
    fn test_nonzero_ffprobe_exit_is_a_successful_no_stream_result() {
        let output = MimeCommandOutput {
            exit_code: Some(1),
            stdout: b"ffprobe could not parse this file".to_vec(),
            stdout_truncated: false,
            stdout_complete: true,
        };
        assert_eq!(MediaStreamType::None, stream_type_from_output(&output).unwrap());
    }

    #[test]
    fn test_incomplete_successful_ffprobe_output_is_rejected() {
        let output = MimeCommandOutput {
            exit_code: Some(0),
            stdout: b"stream listing".to_vec(),
            stdout_truncated: true,
            stdout_complete: false,
        };
        assert!(matches!(
            stream_type_from_output(&output),
            Err(MimeError::ClassifierBackend { backend, reason })
                if backend == "ffprobe" && reason.contains("truncated")
        ));
    }

    #[test]
    fn test_missing_ffprobe_exit_status_is_a_typed_classifier_error() {
        let output = MimeCommandOutput {
            exit_code: None,
            stdout: Vec::new(),
            stdout_truncated: false,
            stdout_complete: true,
        };

        assert!(matches!(
            stream_type_from_output(&output),
            Err(MimeError::ClassifierBackend { backend, reason })
                if backend == "ffprobe" && reason.contains("without a numeric exit code")
        ));
    }
}
