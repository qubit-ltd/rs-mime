//! Submission helpers that preserve input ownership until admission succeeds.

use std::sync::{Arc, Mutex};

use qubit_operation::OperationScope;

use crate::detector::{
    mime_detection_report::MimeBudgetSummary,
    mime_execution_services::MimeExecutionServices,
    mime_input::{InputExtent, InspectedExtent, OwnedMimeInput},
    mime_operation_options::MimeOperationOptions,
};
use crate::{MimeDetectionReport, MimeError, MimeResult};

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

pub(in crate::detector) async fn detect(
    scope: OperationScope,
    input: OwnedMimeInput,
    _services: Arc<MimeExecutionServices>,
    detector: Arc<dyn crate::MimeDetector>,
    options: MimeOperationOptions,
    _permit: Option<crate::detector::internal::mime_budget::BudgetPermit>,
) -> MimeResult<MimeDetectionReport> {
    let _scope = scope;
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
            let max_bytes = options
                .max_buffer_bytes
                .unwrap_or_else(|| detector.max_buffer_size());
            let selected = detector
                .detect_async_path(&file_system, &path, max_bytes, options.policy)
                .await?;
            (
                selected,
                InspectedExtent::Prefix(max_bytes),
                max_bytes as u64,
                max_bytes,
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
