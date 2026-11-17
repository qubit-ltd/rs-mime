// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Submission helpers that preserve input ownership until admission succeeds.

use std::sync::Arc;
use std::sync::Mutex;

use qubit_fs::read::ReadOptions;
use qubit_operation::OperationScope;

use crate::AsyncMimeDetectorBackend;
use crate::BorrowedMimeInput;
use crate::ContentRequirement;
use crate::MimeDetectionReport;
use crate::MimeError;
use crate::MimeResult;
use crate::detector::mime_detection_report::MimeBudgetSummary;
use crate::detector::mime_execution_services::MimeExecutionServices;
use crate::detector::mime_input::InputExtent;
use crate::detector::mime_input::OwnedMimeInput;
use crate::detector::mime_operation::MimeSubmitRejection;
use crate::detector::mime_operation_options::MimeOperationOptions;

/// Shared one-time ownership cell used to return an input after rejection.
pub(in crate::detector) type InputCell = Arc<Mutex<Option<OwnedMimeInput>>>;

/// Places an owned MIME input into a shared one-time cell.
///
/// # Parameters
/// - `input`: Input whose ownership is transferred into the cell.
///
/// # Returns
/// A cloneable cell containing the input.
pub(in crate::detector) fn cell(input: OwnedMimeInput) -> InputCell {
    Arc::new(Mutex::new(Some(input)))
}

/// Takes the input from a cell after a submission has been accepted.
///
/// # Parameters
/// - `cell`: Shared cell from which to take the input.
///
/// # Returns
/// The input removed from the cell.
///
/// # Panics
/// Panics if the input has already been taken.
pub(in crate::detector) fn take(cell: &InputCell) -> OwnedMimeInput {
    cell.lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
        .expect("MIME input is taken once")
}

/// Retrieves the input when a submission is rejected before body execution.
///
/// # Parameters
/// - `cell`: Shared cell containing the unconsumed input.
///
/// # Returns
/// The rejected input that remained unconsumed.
///
/// # Panics
/// Panics if the rejected input is no longer present.
pub(in crate::detector) fn return_input(cell: &InputCell) -> OwnedMimeInput {
    cell.lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
        .expect("rejected MIME body remains uncalled")
}

/// Applies identical option, buffer and shared-memory checks to sync and async
/// submissions before either operation is admitted.
///
/// # Parameters
/// - `input`: Input to validate and reserve.
/// - `options`: Per-operation byte limits.
/// - `requirement`: Whether the backend needs a prefix or complete content.
/// - `random_access`: Provider's random-access capability.
/// - `max_buffer_size`: Service-wide maximum buffer size.
/// - `services`: Shared execution budgets and service limits.
///
/// # Returns
/// The reservation held for the accepted operation.
///
/// # Errors
/// Returns the submission rejection when options are invalid or required
/// memory exceeds the available budget.
pub(in crate::detector) fn preflight(
    input: &OwnedMimeInput,
    options: &MimeOperationOptions,
    requirement: ContentRequirement,
    random_access: crate::RandomAccessCapability,
    max_buffer_size: usize,
    services: &MimeExecutionServices,
) -> Result<crate::detector::internal::mime_budget::BudgetPermit, MimeSubmitRejection> {
    if options.max_buffer_bytes == Some(0) || options.max_staging_bytes == Some(0) || options.max_read_bytes == Some(0)
    {
        return Err(MimeSubmitRejection::InvalidOptions("a request budget is zero".into()));
    }
    let limit = options.max_buffer_bytes.unwrap_or(max_buffer_size).min(max_buffer_size);
    match input {
        OwnedMimeInput::Bytes { content, .. } => {
            if content.len() > limit {
                return Err(MimeSubmitRejection::BudgetExceeded);
            }
            services.reserve_allocation(content)
        }
        OwnedMimeInput::Provider { .. } => {
            if matches!(requirement, ContentRequirement::Complete)
                && random_access == crate::RandomAccessCapability::Unsupported
            {
                // Complete provider input remains owned by the backend's
                // bounded staging implementation, so no prefix buffer is
                // reserved by this layer.
                services.reserve_owned(0)
            } else {
                let read_limit = options
                    .max_read_bytes
                    .unwrap_or(services.max_read_bytes)
                    .min(services.max_read_bytes)
                    .min(u64::try_from(limit).unwrap_or(u64::MAX));
                let read_limit = usize::try_from(read_limit).unwrap_or(usize::MAX);
                services.reserve_owned(read_limit)
            }
        }
    }
    .ok_or(MimeSubmitRejection::BudgetExceeded)
}

/// All state retained for one admitted detection request.
pub(in crate::detector) struct MimeSubmission {
    /// Operation scope used for cancellation and read-tree binding.
    pub scope: OperationScope,
    /// Owned bytes or provider path submitted for detection.
    pub input: OwnedMimeInput,
    /// Shared execution services and byte limits.
    pub services: Arc<MimeExecutionServices>,
    /// Detector implementation to invoke.
    pub backend: Arc<dyn AsyncMimeDetectorBackend>,
    /// Identifier recorded in the returned report.
    pub backend_id: String,
    /// Service-wide maximum content buffer.
    pub max_buffer_size: usize,
    /// Per-operation detection and resource limits.
    pub options: MimeOperationOptions,
    /// Owned-memory reservation retained during detection.
    pub permit: Option<crate::detector::internal::mime_budget::BudgetPermit>,
    /// Read accounting context for provider-backed input.
    pub read_tree: Arc<crate::detector::internal::mime_budget::ReadTreeContext>,
}

/// Runs an admitted detection request and assembles its result report.
///
/// # Parameters
/// - `request`: State for the accepted input, backend, services, and limits.
///
/// # Returns
/// The MIME result report with the detected type and resource usage.
///
/// # Errors
/// Propagates backend, input-provider, and configured-limit errors.
pub(in crate::detector) async fn detect(request: MimeSubmission) -> MimeResult<MimeDetectionReport> {
    let MimeSubmission {
        scope,
        input,
        services,
        backend,
        backend_id,
        max_buffer_size,
        options,
        permit,
        read_tree,
    } = request;
    services.bind_read_tree(scope.id(), &read_tree);
    let _scope = scope;
    let _permit = permit;
    let (mime_type, extent, bytes_read, owned_bytes, staging_bytes) = match input {
        OwnedMimeInput::Bytes {
            content,
            extent,
            filename,
        } => {
            if let Some(limit) = options.max_buffer_bytes
                && content.len() > limit
            {
                return Err(MimeError::BufferLimitExceeded {
                    requested: content.len(),
                    limit,
                });
            }
            let candidates = backend
                .guess_content_in(
                    &_scope,
                    BorrowedMimeInput::Bytes {
                        content: &content,
                        extent,
                        filename: filename.as_deref(),
                    },
                    &options,
                )
                .await?;
            (
                candidates
                    .candidates
                    .first()
                    .map(|candidate| candidate.mime_type.clone()),
                candidates.inspected_extent,
                0,
                content.len(),
                candidates.staging_bytes.max(read_tree.staging_usage().peak()),
            )
        }
        OwnedMimeInput::Provider { file_system, path } => {
            let candidates = if matches!(backend.content_requirement(), ContentRequirement::Complete)
                || backend.random_access_capability() == crate::RandomAccessCapability::SamplesSameObject
            {
                backend
                    .guess_content_in(
                        &_scope,
                        BorrowedMimeInput::Provider {
                            file_system: &file_system,
                            path: &path,
                        },
                        &options,
                    )
                    .await?
            } else {
                let read_limit = options
                    .max_read_bytes
                    .unwrap_or(services.max_read_bytes)
                    .min(services.max_read_bytes);
                let max_bytes = options
                    .max_buffer_bytes
                    .unwrap_or(max_buffer_size)
                    .min(max_buffer_size)
                    .min(match backend.content_requirement() {
                        ContentRequirement::Prefix(required) => required,
                        ContentRequirement::Complete => max_buffer_size,
                    })
                    .min(usize::try_from(read_limit).unwrap_or(usize::MAX));
                let read_permit = read_tree.reserve(max_bytes as u64).ok_or(MimeError::BudgetExceeded {
                    kind: "read",
                    requested: max_bytes as u64,
                    limit: read_tree.remaining(),
                })?;
                let outcome = file_system.read_prefix(&path, ReadOptions::default(), max_bytes).await;
                let content = match outcome {
                    Ok(outcome) => outcome.into_bytes(),
                    Err(error) => {
                        read_permit.commit(max_bytes as u64);
                        return Err(error.into());
                    }
                };
                let bytes_read = content.len() as u64;
                let _read = read_permit.commit(bytes_read);
                let candidates = backend
                    .guess_content_in(
                        &_scope,
                        BorrowedMimeInput::Bytes {
                            content: &content,
                            extent: InputExtent::Prefix,
                            filename: path.file_name(),
                        },
                        &options,
                    )
                    .await?;
                return Ok(MimeDetectionReport {
                    mime_type: candidates
                        .candidates
                        .first()
                        .map(|candidate| candidate.mime_type.clone()),
                    backend_id,
                    inspected_extent: candidates.inspected_extent,
                    budget: MimeBudgetSummary {
                        owned_bytes: content.len(),
                        bytes_read,
                        staging_bytes: candidates.staging_bytes.max(read_tree.staging_usage().peak()),
                    },
                });
            };
            (
                candidates
                    .candidates
                    .first()
                    .map(|candidate| candidate.mime_type.clone()),
                candidates.inspected_extent,
                read_tree.consumed(),
                0,
                candidates.staging_bytes.max(read_tree.staging_usage().peak()),
            )
        }
    };
    Ok(MimeDetectionReport {
        mime_type,
        backend_id,
        inspected_extent: extent,
        budget: MimeBudgetSummary {
            owned_bytes,
            bytes_read,
            staging_bytes,
        },
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::cell;
    use super::preflight;
    use super::return_input;
    use super::take;
    use crate::ContentRequirement;
    use crate::MimeError;
    use crate::MimeOperationOptions;
    use crate::RandomAccessCapability;
    use crate::detector::MimeExecutionPort;
    use crate::detector::MimeExecutionServices;
    use crate::detector::mime_input::InputExtent;
    use crate::detector::mime_input::OwnedMimeInput;
    use crate::detector::mime_operation::MimeSubmitRejection;

    #[derive(Debug)]
    struct NoopPort;

    impl MimeExecutionPort for NoopPort {
        fn prepare(&self) -> Result<(), MimeError> {
            Ok(())
        }

        fn shutdown(&self) -> Result<(), MimeError> {
            Ok(())
        }
    }

    fn services() -> Arc<MimeExecutionServices> {
        let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
        let group = manager.group(qubit_operation::GroupOptions { capacity: 1 }).unwrap();
        Arc::new(MimeExecutionServices::new(manager, group, Arc::new(NoopPort), 32).unwrap())
    }

    fn bytes_input() -> OwnedMimeInput {
        OwnedMimeInput::bytes(Arc::from(b"payload".as_slice()), InputExtent::Complete).with_filename("report.bin")
    }

    fn assert_original_input(input: OwnedMimeInput) {
        match input {
            OwnedMimeInput::Bytes {
                content,
                extent,
                filename,
            } => {
                assert_eq!(b"payload", content.as_ref());
                assert_eq!(InputExtent::Complete, extent);
                assert_eq!(Some("report.bin".to_owned()), filename);
            }
            OwnedMimeInput::Provider { .. } => panic!("byte input should retain its original variant"),
        }
    }

    #[test]
    fn test_preflight_reserves_valid_bytes_and_rejects_tight_or_zero_limits() {
        let services = services();
        let input = OwnedMimeInput::bytes(Arc::from(b"payload".as_slice()), InputExtent::Complete);
        let permit = preflight(
            &input,
            &MimeOperationOptions::default(),
            ContentRequirement::Prefix(8),
            RandomAccessCapability::Unsupported,
            16,
            &services,
        )
        .expect("valid in-memory input should reserve its owned bytes");
        drop(permit);

        let too_small = MimeOperationOptions {
            max_buffer_bytes: Some(4),
            ..Default::default()
        };
        assert!(matches!(
            preflight(
                &input,
                &too_small,
                ContentRequirement::Prefix(8),
                RandomAccessCapability::Unsupported,
                16,
                &services,
            ),
            Err(MimeSubmitRejection::BudgetExceeded)
        ));

        let zero = MimeOperationOptions {
            max_read_bytes: Some(0),
            ..Default::default()
        };
        assert!(matches!(
            preflight(
                &input,
                &zero,
                ContentRequirement::Prefix(8),
                RandomAccessCapability::Unsupported,
                16,
                &services,
            ),
            Err(MimeSubmitRejection::InvalidOptions(_))
        ));
    }

    #[test]
    fn test_rejected_input_can_be_recovered_after_cell_lock_poisoning() {
        let input = cell(bytes_input());
        let other = Arc::clone(&input);
        assert!(
            std::thread::spawn(move || {
                let _guard = other.lock().unwrap();
                panic!("poison input cell for ownership recovery test");
            })
            .join()
            .is_err()
        );
        assert_original_input(return_input(&input));
    }

    #[test]
    fn test_take_moves_input_out_of_cell_once() {
        let input = cell(bytes_input());
        assert_original_input(take(&input));
        assert!(input.lock().unwrap().is_none());
    }

    #[test]
    fn test_accepted_input_can_be_taken_after_cell_lock_poisoning() {
        let input = cell(bytes_input());
        let other = Arc::clone(&input);
        assert!(
            std::thread::spawn(move || {
                let _guard = other.lock().unwrap();
                panic!("poison input cell before accepted ownership transfer");
            })
            .join()
            .is_err()
        );
        assert_original_input(take(&input));
    }

    #[test]
    fn test_return_input_preserves_rejected_request_ownership() {
        let input = cell(bytes_input());
        assert_original_input(return_input(&input));
        assert!(input.lock().unwrap().is_none());
    }
}
