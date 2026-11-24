// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Submission helpers that preserve input ownership until admission succeeds.

use std::future::Future;
use std::sync::Arc;
use std::sync::Mutex;
use std::task::Poll;

use prefix_read_accounting::PrefixReadAccounting;
use qubit_fs::read::ObservedReadError;
use qubit_fs::read::ReadOptions;
use qubit_operation::OperationScope;

pub(in crate::detector) use super::provider_read_plan::ProviderReadPlan;

mod prefix_read_accounting;

use crate::AsyncMimeDetectorBackend;
use crate::BorrowedMimeInput;
use crate::ContentRequirement;
use crate::MeteredReadSession;
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
    managed_provider_reads: bool,
) -> Result<crate::detector::internal::mime_budget::BudgetPermit, MimeSubmitRejection> {
    if options.max_buffer_bytes == Some(0)
        || options.max_staging_bytes == Some(0)
        || options.max_read_bytes == Some(0)
    {
        return Err(MimeSubmitRejection::InvalidOptions(
            "a request budget is zero".into(),
        ));
    }
    let limit = options
        .max_buffer_bytes
        .unwrap_or(max_buffer_size)
        .min(max_buffer_size);
    match input {
        OwnedMimeInput::Bytes { content, .. } => {
            if content.len() > limit {
                return Err(MimeSubmitRejection::BudgetExceeded);
            }
            services.reserve_allocation(content)
        }
        OwnedMimeInput::Provider { .. } => {
            if managed_provider_reads {
                // Managed readers reserve only their bounded scratch buffer
                // around each read; they do not retain a whole input payload.
                services.reserve_owned(0)
            } else if matches!(requirement, ContentRequirement::Complete)
                && random_access == crate::RandomAccessCapability::Unsupported
            {
                // Complete provider input remains owned by the backend's
                // bounded staging implementation, so no prefix buffer is
                // reserved by this layer.
                services.reserve_owned(0)
            } else if matches!(requirement, ContentRequirement::Prefix(_))
                && random_access != crate::RandomAccessCapability::SamplesSameObject
            {
                services.reserve_owned(prefix_buffer_bytes(
                    options,
                    match requirement {
                        ContentRequirement::Prefix(required) => required,
                        ContentRequirement::Complete => {
                            unreachable!("complete content was handled above")
                        }
                    },
                    max_buffer_size,
                    services.max_read_bytes(),
                ))
            } else {
                let read_limit = options
                    .max_read_bytes
                    .unwrap_or(services.max_read_bytes())
                    .min(services.max_read_bytes())
                    .min(u64::try_from(limit).unwrap_or(u64::MAX));
                let read_limit = usize::try_from(read_limit).unwrap_or(usize::MAX);
                services.reserve_owned(read_limit)
            }
        }
    }
    .ok_or(MimeSubmitRejection::BudgetExceeded)
}

/// Calculates the exact prefix allocation admitted and later requested from a
/// non-random-access provider.
///
/// # Parameters
/// - `options`: Per-request buffer and read limits.
/// - `required_prefix`: Prefix length required by the backend.
/// - `max_buffer_size`: Service-wide maximum buffer size.
/// - `service_read_limit`: Service-wide cumulative read limit.
///
/// # Returns
/// The prefix allocation bounded by every request and service limit.
fn prefix_buffer_bytes(
    options: &MimeOperationOptions,
    required_prefix: usize,
    max_buffer_size: usize,
    service_read_limit: u64,
) -> usize {
    let request_buffer_limit = options.max_buffer_bytes.unwrap_or(max_buffer_size);
    let request_read_limit = options.max_read_bytes.unwrap_or(service_read_limit);
    let read_limit = request_read_limit.min(service_read_limit);
    request_buffer_limit
        .min(max_buffer_size)
        .min(required_prefix)
        .min(usize::try_from(read_limit).unwrap_or(usize::MAX))
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
    /// Immutable backend read strategy shared by preflight and execution.
    pub read_plan: ProviderReadPlan,
    /// Identifier recorded in the returned report.
    pub backend_id: String,
    /// Unique repository filename winner determined during submission
    /// preflight, if policy permits skipping content inspection.
    pub filename_winner: Option<String>,
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
pub(in crate::detector) async fn detect(
    request: MimeSubmission,
) -> MimeResult<MimeDetectionReport> {
    let context = request.scope.context();
    let mut stop = Box::pin(context.stop_requested());
    let mut deadline = context.deadline_signal().map(Box::pin);
    let mut detection = Box::pin(detect_inner(request));
    std::future::poll_fn(move |cx| {
        if let Some(deadline) = deadline.as_mut()
            && deadline.as_mut().poll(cx).is_ready()
        {
            return Poll::Ready(Err(MimeError::detector_backend(
                "mime",
                "operation deadline expired",
            )));
        }
        if let Poll::Ready(reason) = stop.as_mut().poll(cx) {
            return Poll::Ready(Err(MimeError::detector_backend(
                "mime",
                format!("operation stopped: {reason:?}"),
            )));
        }
        let result = detection.as_mut().poll(cx);
        if result.is_ready() {
            if let Some(reason) = context.stop_reason() {
                return Poll::Ready(Err(MimeError::detector_backend(
                    "mime",
                    format!("operation stopped: {reason:?}"),
                )));
            }
            if let Some(deadline) = deadline.as_mut()
                && deadline.as_mut().poll(cx).is_ready()
            {
                return Poll::Ready(Err(MimeError::detector_backend(
                    "mime",
                    "operation deadline expired",
                )));
            }
        }
        result
    })
    .await
}

/// Executes the detection body after its stop and deadline signals are wired.
///
/// # Parameters
/// - `request`: Accepted detection state with its backend and resource budget.
///
/// # Returns
/// The MIME result report with detected type and resource usage.
///
/// # Errors
/// Propagates backend, input-provider, stop, and configured-limit errors.
async fn detect_inner(request: MimeSubmission) -> MimeResult<MimeDetectionReport> {
    let MimeSubmission {
        scope,
        input,
        services,
        backend,
        read_plan,
        backend_id,
        filename_winner,
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
            if let Some(mime_type) = filename_winner.as_deref() {
                if let Some(reason) = _scope.context().stop_reason() {
                    return Err(MimeError::DetectorBackend {
                        backend: backend_id,
                        reason: format!("operation stopped: {reason:?}"),
                    });
                }
                return Ok(MimeDetectionReport {
                    mime_type: Some(mime_type.to_owned()),
                    backend_id,
                    inspected_extent: crate::InspectedExtent::Prefix(0),
                    budget: MimeBudgetSummary {
                        owned_bytes: content.len(),
                        ..Default::default()
                    },
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
                read_tree.bytes_read(),
                content.len(),
                candidates
                    .staging_bytes
                    .max(read_tree.staging_usage().peak()),
            )
        }
        OwnedMimeInput::Provider { file_system, path } => {
            if let Some(mime_type) = filename_winner.as_deref() {
                if let Some(reason) = _scope.context().stop_reason() {
                    return Err(MimeError::DetectorBackend {
                        backend: backend_id,
                        reason: format!("operation stopped: {reason:?}"),
                    });
                }
                return Ok(MimeDetectionReport {
                    mime_type: Some(mime_type.to_owned()),
                    backend_id,
                    inspected_extent: crate::InspectedExtent::Prefix(0),
                    budget: MimeBudgetSummary::default(),
                });
            }
            if read_plan.managed_provider_reads {
                let request_limit = options
                    .max_read_bytes
                    .unwrap_or(services.max_read_bytes)
                    .min(services.max_read_bytes);
                let payload_limit = options
                    .max_buffer_bytes
                    .unwrap_or(max_buffer_size)
                    .min(max_buffer_size);
                let mut session = if read_plan.random_access
                    == crate::RandomAccessCapability::SamplesSameObject
                {
                    MeteredReadSession::open_random_access(
                        &file_system,
                        &path,
                        Arc::clone(&services),
                        Arc::clone(&read_tree),
                        request_limit,
                        payload_limit,
                    )
                    .await?
                } else {
                    MeteredReadSession::open_sequential(
                        &file_system,
                        &path,
                        Arc::clone(&services),
                        Arc::clone(&read_tree),
                        request_limit,
                        payload_limit,
                    )
                    .await?
                };
                let candidates = backend
                    .guess_provider_content_in(
                        &_scope,
                        BorrowedMimeInput::ManagedProvider {
                            session: &mut session,
                            filename: path.file_name(),
                        },
                        path.file_name(),
                        &options,
                    )
                    .await?;
                (
                    candidates
                        .candidates
                        .first()
                        .map(|candidate| candidate.mime_type.clone()),
                    candidates.inspected_extent,
                    read_tree.bytes_read(),
                    0,
                    candidates
                        .staging_bytes
                        .max(read_tree.staging_usage().peak()),
                )
            } else {
                let candidates = if matches!(read_plan.requirement, ContentRequirement::Complete)
                    || read_plan.random_access == crate::RandomAccessCapability::SamplesSameObject
                {
                    backend
                        .guess_provider_content_in(
                            &_scope,
                            BorrowedMimeInput::Provider {
                                file_system: &file_system,
                                path: &path,
                            },
                            path.file_name(),
                            &options,
                        )
                        .await?
                } else {
                    let max_bytes = prefix_buffer_bytes(
                        &options,
                        match read_plan.requirement {
                            ContentRequirement::Prefix(required) => required,
                            ContentRequirement::Complete => {
                                unreachable!("complete content was handled above")
                            }
                        },
                        max_buffer_size,
                        services.max_read_bytes(),
                    );
                    let read_permit =
                        read_tree
                            .reserve(max_bytes as u64)
                            .ok_or(MimeError::BudgetExceeded {
                                kind: "read",
                                requested: max_bytes as u64,
                                limit: read_tree.remaining(),
                            })?;
                    let mut read_accounting =
                        PrefixReadAccounting::new(read_permit, _scope.context());
                    let outcome = file_system
                        .read_prefix_observed(
                            &path,
                            ReadOptions::default(),
                            max_bytes,
                            &mut read_accounting,
                        )
                        .await;
                    let _read = read_accounting.commit();
                    let bytes_read = read_tree.bytes_read();
                    let content = outcome
                        .map_err(|error| match error {
                            ObservedReadError::Stopped => MimeError::detector_backend(
                                "mime",
                                "operation stopped before provider I/O",
                            ),
                            ObservedReadError::Provider(error) => MimeError::from(error),
                        })?
                        .into_bytes();
                    let candidates = backend
                        .guess_provider_content_in(
                            &_scope,
                            BorrowedMimeInput::Bytes {
                                content: &content,
                                extent: InputExtent::Prefix,
                                filename: path.file_name(),
                            },
                            path.file_name(),
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
                            staging_bytes: candidates
                                .staging_bytes
                                .max(read_tree.staging_usage().peak()),
                        },
                    });
                };
                (
                    candidates
                        .candidates
                        .first()
                        .map(|candidate| candidate.mime_type.clone()),
                    candidates.inspected_extent,
                    read_tree.bytes_read(),
                    0,
                    candidates
                        .staging_bytes
                        .max(read_tree.staging_usage().peak()),
                )
            }
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
    use std::sync::Mutex;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;

    use super::ProviderReadPlan;
    use super::cell;
    use super::prefix_buffer_bytes;
    use super::preflight;
    use super::return_input;
    use super::take;
    use crate::AsyncMimeDetectorBackend;
    use crate::BorrowedMimeInput;
    use crate::ContentRequirement;
    use crate::MimeCandidates;
    use crate::MimeError;
    use crate::MimeFuture;
    use crate::MimeOperationOptions;
    use crate::RandomAccessCapability;
    use crate::detector::MimeExecutionPort;
    use crate::detector::MimeExecutionServices;
    use crate::detector::mime_input::InputExtent;
    use crate::detector::mime_input::OwnedMimeInput;
    use crate::detector::mime_operation::MimeSubmitRejection;

    #[derive(Debug, Default)]
    struct CapabilityHooks {
        requirement: AtomicUsize,
        random_access: AtomicUsize,
        managed: AtomicUsize,
    }

    impl AsyncMimeDetectorBackend for CapabilityHooks {
        fn content_requirement(&self) -> ContentRequirement {
            self.requirement.fetch_add(1, Ordering::SeqCst);
            ContentRequirement::Complete
        }

        fn random_access_capability(&self) -> RandomAccessCapability {
            self.random_access.fetch_add(1, Ordering::SeqCst);
            RandomAccessCapability::Unsupported
        }

        fn uses_managed_provider_reads(&self) -> bool {
            self.managed.fetch_add(1, Ordering::SeqCst);
            false
        }

        fn guess_content_in<'a>(
            &'a self,
            _scope: &'a qubit_operation::OperationScope,
            _input: BorrowedMimeInput<'a>,
            _options: &'a MimeOperationOptions,
        ) -> MimeFuture<'a, MimeCandidates> {
            Box::pin(async {
                Ok(MimeCandidates {
                    candidates: Vec::new(),
                    inspected_extent: crate::InspectedExtent::Prefix(0),
                    retained_prefix: Vec::new(),
                    staging_bytes: 0,
                })
            })
        }
    }

    #[derive(Debug, Default)]
    struct NoopPort {
        events: Arc<Mutex<Vec<&'static str>>>,
    }

    impl MimeExecutionPort for NoopPort {
        fn prepare(&self) -> Result<(), MimeError> {
            self.events.lock().unwrap().push("prepare");
            Ok(())
        }

        fn shutdown(&self) -> Result<(), MimeError> {
            self.events.lock().unwrap().push("shutdown");
            Ok(())
        }
    }

    fn services() -> Arc<MimeExecutionServices> {
        let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
        let group = manager
            .group(qubit_operation::GroupOptions { capacity: 1 })
            .unwrap();
        Arc::new(
            MimeExecutionServices::new(manager, group, Arc::new(NoopPort::default()), 32).unwrap(),
        )
    }

    #[test]
    fn test_provider_read_plan_captures_hooks_and_enforces_complete_reads() {
        let backend = CapabilityHooks::default();
        let plan = ProviderReadPlan::capture(&backend, false);

        assert_eq!(1, backend.requirement.load(Ordering::SeqCst));
        assert_eq!(1, backend.random_access.load(Ordering::SeqCst));
        assert_eq!(1, backend.managed.load(Ordering::SeqCst));
        assert!(plan.managed_provider_reads);
        assert_eq!(ContentRequirement::Complete, plan.requirement);
    }

    #[test]
    fn test_trusted_file_keeps_self_metered_complete_reads() {
        let backend = CapabilityHooks::default();
        let plan = ProviderReadPlan::capture(&backend, true);

        assert!(!plan.managed_provider_reads);
    }

    fn bytes_input() -> OwnedMimeInput {
        OwnedMimeInput::bytes(Arc::from(b"payload".as_slice()), InputExtent::Complete)
            .with_filename("report.bin")
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
            OwnedMimeInput::Provider { .. } => {
                panic!("byte input should retain its original variant")
            }
        }
    }

    #[test]
    fn test_preflight_reserves_valid_bytes_and_rejects_tight_or_zero_limits() {
        let port = NoopPort::default();
        port.prepare().unwrap();
        port.shutdown().unwrap();
        assert_eq!(vec!["prepare", "shutdown"], *port.events.lock().unwrap());

        let services = services();
        let input = OwnedMimeInput::bytes(Arc::from(b"payload".as_slice()), InputExtent::Complete);
        let permit = preflight(
            &input,
            &MimeOperationOptions::default(),
            ContentRequirement::Prefix(8),
            RandomAccessCapability::Unsupported,
            16,
            &services,
            false,
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
                false,
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
                false,
            ),
            Err(MimeSubmitRejection::InvalidOptions(_))
        ));
    }

    #[test]
    fn test_prefix_buffer_bytes_uses_required_request_and_service_limits() {
        let options = MimeOperationOptions::default();
        assert_eq!(
            4,
            prefix_buffer_bytes(&options, 4, 16 * 1024 * 1024, 16 * 1024 * 1024)
        );
        assert_eq!(
            0,
            prefix_buffer_bytes(&options, 0, 16 * 1024 * 1024, 16 * 1024 * 1024)
        );

        let tight_buffer = MimeOperationOptions {
            max_buffer_bytes: Some(2),
            ..Default::default()
        };
        assert_eq!(2, prefix_buffer_bytes(&tight_buffer, 4, 16, 64));
        let tight_read = MimeOperationOptions {
            max_read_bytes: Some(3),
            ..Default::default()
        };
        assert_eq!(3, prefix_buffer_bytes(&tight_read, 4, 16, 64));
        assert_eq!(4, prefix_buffer_bytes(&options, 8, 16, 4));
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
