//! Submission helpers that preserve input ownership until admission succeeds.

use std::sync::{Arc, Mutex};

use qubit_operation::OperationScope;

use crate::detector::{
    mime_detection_report::MimeBudgetSummary,
    mime_execution_services::MimeExecutionServices,
    mime_input::{InputExtent, InspectedExtent, OwnedMimeInput},
    mime_operation::MimeSubmitRejection,
    mime_operation_options::MimeOperationOptions,
};
use crate::{ContentRequirement, MimeDetectionReport, MimeError, MimeResult};

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
    detector: &dyn crate::MimeDetector,
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
        .unwrap_or_else(|| detector.max_buffer_size())
        .min(detector.max_buffer_size());
    match input {
        OwnedMimeInput::Bytes { content, .. } => {
            if content.len() > limit {
                return Err(MimeSubmitRejection::BudgetExceeded);
            }
            services.reserve_allocation(content)
        }
        OwnedMimeInput::Provider { .. } => {
            let read_limit = options
                .max_read_bytes
                .unwrap_or(services.max_read_bytes)
                .min(services.max_read_bytes)
                .min(u64::try_from(limit).unwrap_or(u64::MAX));
            let read_limit = usize::try_from(read_limit).unwrap_or(usize::MAX);
            services.reserve_owned(read_limit)
        }
    }
    .ok_or(MimeSubmitRejection::BudgetExceeded)
}

pub(in crate::detector) async fn detect(
    scope: OperationScope,
    input: OwnedMimeInput,
    services: Arc<MimeExecutionServices>,
    detector: Arc<dyn crate::MimeDetector>,
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
            let selected = detector.detect(&content, filename.as_deref(), options.policy)?;
            let inspected = match extent {
                InputExtent::Complete => InspectedExtent::Complete,
                InputExtent::Prefix => InspectedExtent::Prefix(content.len()),
            };
            (selected, inspected, 0, content.len())
        }
        OwnedMimeInput::Provider { file_system, path } => {
            if matches!(detector.content_requirement(), ContentRequirement::Complete) {
                return Err(MimeError::CompleteContentRequired);
            }
            let max_bytes = options
                .max_buffer_bytes
                .unwrap_or_else(|| detector.max_buffer_size())
                .min(detector.max_buffer_size())
                .min(
                    usize::try_from(
                        options
                            .max_read_bytes
                            .unwrap_or(services.max_read_bytes)
                            .min(services.max_read_bytes),
                    )
                    .unwrap_or(usize::MAX),
                );
            let read_permit =
                read_tree
                    .reserve(max_bytes as u64)
                    .ok_or(MimeError::BudgetExceeded {
                        kind: "read",
                        requested: max_bytes as u64,
                        limit: read_tree.remaining(),
                    })?;
            let read_outcome = file_system
                .read_prefix(&path, qubit_fs::read::ReadOptions::default(), max_bytes)
                .await;
            let content = match read_outcome {
                Ok(outcome) => outcome.into_bytes(),
                Err(error) => {
                    // The facade does not expose partial bytes on failure, so
                    // retain the reservation conservatively after provider I/O.
                    read_permit.commit(max_bytes as u64);
                    return Err(error.into());
                }
            };
            let bytes_read = content.len() as u64;
            let _actual_read = read_permit.commit(bytes_read);
            let selected = detector.detect_prefix(&content, path.file_name(), options.policy)?;
            (
                selected,
                InspectedExtent::Prefix(content.len()),
                bytes_read,
                content.len(),
            )
        }
    };
    Ok(MimeDetectionReport {
        mime_type,
        backend_id: "repository".to_owned(),
        inspected_extent: extent,
        budget: MimeBudgetSummary {
            owned_bytes,
            bytes_read,
            staging_bytes: 0,
        },
    })
}
