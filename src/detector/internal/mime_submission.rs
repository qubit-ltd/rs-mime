//! Submission helpers that preserve input ownership until admission succeeds.

use std::sync::{Arc, Mutex};

use qubit_operation::OperationScope;

use crate::detector::{
    mime_detection_report::MimeBudgetSummary,
    mime_execution_services::MimeExecutionServices,
    mime_input::{InputExtent, OwnedMimeInput},
    mime_operation::MimeSubmitRejection,
    mime_operation_options::MimeOperationOptions,
};
use crate::{
    AsyncMimeDetectorBackend, BorrowedMimeInput, ContentRequirement, MimeDetectionReport,
    MimeError, MimeResult,
};

pub(in crate::detector) type InputCell = Arc<Mutex<Option<OwnedMimeInput>>>;

pub(in crate::detector) fn cell(input: OwnedMimeInput) -> InputCell {
    Arc::new(Mutex::new(Some(input)))
}

pub(in crate::detector) fn take(cell: &InputCell) -> OwnedMimeInput {
    cell.lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
        .expect("MIME input is taken once")
}

pub(in crate::detector) fn return_input(cell: &InputCell) -> OwnedMimeInput {
    cell.lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
        .expect("rejected MIME body remains uncalled")
}

/// Applies identical option, buffer and shared-memory checks to sync and async
/// submissions before either operation is admitted.
pub(in crate::detector) fn preflight(
    input: &OwnedMimeInput,
    options: &MimeOperationOptions,
    requirement: ContentRequirement,
    random_access: crate::RandomAccessCapability,
    max_buffer_size: usize,
    services: &MimeExecutionServices,
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

pub(in crate::detector) async fn detect(
    scope: OperationScope,
    input: OwnedMimeInput,
    services: Arc<MimeExecutionServices>,
    backend: Arc<dyn AsyncMimeDetectorBackend>,
    backend_id: String,
    max_buffer_size: usize,
    options: MimeOperationOptions,
    permit: Option<crate::detector::internal::mime_budget::BudgetPermit>,
    read_tree: Arc<crate::detector::internal::mime_budget::ReadTreeContext>,
) -> MimeResult<MimeDetectionReport> {
    services.bind_read_tree(scope.id(), &read_tree);
    let _scope = scope;
    let _permit = permit;
    let (mime_type, extent, bytes_read, owned_bytes) = match input {
        OwnedMimeInput::Bytes {
            content,
            extent,
            filename,
        } => {
            if let Some(limit) = options.max_buffer_bytes {
                if content.len() > limit {
                    return Err(MimeError::BufferLimitExceeded {
                        requested: content.len(),
                        limit,
                    });
                }
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
            )
        }
        OwnedMimeInput::Provider { file_system, path } => {
            let candidates =
                if matches!(backend.content_requirement(), ContentRequirement::Complete)
                    || backend.random_access_capability()
                        == crate::RandomAccessCapability::SamplesSameObject
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
                    let read_permit =
                        read_tree
                            .reserve(max_bytes as u64)
                            .ok_or(MimeError::BudgetExceeded {
                                kind: "read",
                                requested: max_bytes as u64,
                                limit: read_tree.remaining(),
                            })?;
                    let outcome = file_system
                        .read_prefix(&path, qubit_fs::read::ReadOptions::default(), max_bytes)
                        .await;
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
                            staging_bytes: 0,
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
            staging_bytes: 0,
        },
    })
}
