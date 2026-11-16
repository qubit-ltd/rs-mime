// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Managed temporary-file staging for MIME command inputs.

use std::sync::Arc;
use std::sync::Mutex;

use qubit_fs::AsyncFileSystem;
use qubit_fs::Path as FsPath;
use qubit_fs::read::ReadOptions;
use qubit_io::AsyncInput;
use qubit_local_files::LocalFileSystem;
use qubit_operation::CleanupFailure;
use qubit_operation::CleanupStage;
use qubit_operation::DiagnosticSummary;
use qubit_operation::OperationScope;
use qubit_operation::ResourceId;
use qubit_operation::ResourceRef;
use qubit_operation::ResourceReservation;
use qubit_operation::StopReason;
use qubit_operation::StopReport;

use crate::MimeError;
use crate::MimeFuture;
use crate::MimeResult;
use crate::command_execution::MimeExecutionWorker;
use crate::detector::OwnedMemoryPermit;
use crate::detector::ReadPermit;
use crate::detector::ReadTreeContext;
use crate::detector::StagingPermit;
use crate::detector::mime_input::OwnedMimeInput;

mod completion;
mod managed_staging_limits;
mod managed_temp_cleanup;
mod staged_mime_input;

use completion::completion_pair;
pub(crate) use managed_staging_limits::ManagedStagingLimits;
use managed_temp_cleanup::ManagedTempCleanup;
pub(crate) use staged_mime_input::StagedMimeInput;

/// Stages owned bytes without performing filesystem work in the caller's
/// async poll. A rejected worker dispatch returns the job and drops the
/// reservation before any temporary file is created.
///
/// # Parameters
/// - `scope`: Operation scope that owns the staged resource.
/// - `input`: Owned bytes or provider-backed input to stage.
/// - `limits`: Per-request staging and execution limits.
/// - `read_tree`: Shared provider-read budget context.
///
/// # Returns
/// A future resolving to the operation-owned staged input.
///
/// # Errors
/// Returns an error when staging fails, is stopped, or exceeds its budget.
///
/// # Parameters
/// - `scope`: Operation scope that owns the staged resource.
/// - `input`: Owned bytes or provider-backed input to stage.
/// - `limits`: Per-request staging and execution limits.
/// - `read_tree`: Shared provider-read budget context.
///
/// # Returns
/// A future resolving to the operation-owned staged input.
///
/// # Errors
/// Returns an error when staging fails, is stopped, or exceeds its budget.
pub(crate) fn stage_owned_in<'a>(
    scope: &'a OperationScope,
    input: OwnedMimeInput,
    limits: ManagedStagingLimits,
    read_tree: Arc<ReadTreeContext>,
) -> MimeFuture<'a, ResourceRef<StagedMimeInput>> {
    Box::pin(async move {
        match input {
            OwnedMimeInput::Bytes { content, extent, .. } => stage_byte_slice_in(scope, &content, extent, limits).await,
            OwnedMimeInput::Provider { file_system, path } => {
                stage_provider_in(scope, file_system, path, limits, read_tree).await
            }
        }
    })
}

/// Rejects the next staging step when the operation context requests stop.
///
/// # Parameters
/// - `scope`: Operation scope whose stop state is checked.
///
/// # Returns
/// `Ok(())` when the operation is still active.
///
/// # Errors
/// Returns the operation stop reason when stopping has begun.
///
/// # Parameters
/// - `scope`: Operation scope whose stop state is checked.
///
/// # Returns
/// `Ok(())` when the operation is still active.
///
/// # Errors
/// Returns the operation stop reason when stopping has begun.
fn ensure_not_stopped(scope: &OperationScope) -> MimeResult<()> {
    if let Some(reason) = scope.context().stop_reason() {
        return Err(stopped_error(reason));
    }
    Ok(())
}

/// Converts the operation's stop signal into the MIME error used by staging.
///
/// # Parameters
/// - `reason`: Stop reason reported by the operation context.
///
/// # Returns
/// A MIME error carrying the operation stop report.
///
/// # Parameters
/// - `reason`: Stop reason reported by the operation context.
///
/// # Returns
/// A MIME error carrying the operation stop report.
fn stopped_error(reason: StopReason) -> MimeError {
    MimeError::OperationStopped(StopReport {
        reason,
        accepted_at: None,
    })
}

/// Stages borrowed bytes while the caller's input borrow remains active.
///
/// # Parameters
/// - `scope`: Operation scope that owns the staged resource.
/// - `content`: Borrowed bytes to copy into the staged file.
/// - `extent`: Completeness claim for the input; staging preserves the bounded
///   byte slice supplied by the caller.
/// - `limits`: Per-request staging and execution limits.
///
/// # Returns
/// A future resolving to the operation-owned staged input.
///
/// # Errors
/// Returns an error when the operation stops, a budget is exceeded, or staging
/// work cannot be dispatched or completed.
pub(crate) fn stage_borrowed_bytes_in<'a>(
    scope: &'a OperationScope,
    content: &'a [u8],
    extent: crate::InputExtent,
    limits: ManagedStagingLimits,
) -> MimeFuture<'a, ResourceRef<StagedMimeInput>> {
    Box::pin(stage_byte_slice_in(scope, content, extent, limits))
}

/// Copies a bounded byte slice to an operation-owned temporary file.
///
/// Each chunk reserves staging and temporary owned-memory capacity before a
/// worker writes it. The resource reservation keeps cleanup responsible for
/// any partial file if a later chunk fails.
///
/// # Parameters
/// - `scope`: Operation scope that owns the staged resource.
/// - `content`: Bounded input bytes to copy.
/// - `_extent`: Completeness claim retained for the staging contract.
/// - `limits`: Per-request staging and execution limits.
///
/// # Returns
/// The registered staged-file resource.
///
/// # Errors
/// Returns an error when writing, dispatch, or a budget check fails.
///
/// # Parameters
/// - `scope`: Operation scope that owns the staged resource.
/// - `content`: Bounded input bytes to copy.
/// - `_extent`: Completeness claim retained for the staging contract.
/// - `limits`: Per-request staging and execution limits.
///
/// # Returns
/// The registered staged-file resource.
///
/// # Errors
/// Returns an error when writing, dispatch, or a budget check fails.
async fn stage_byte_slice_in(
    scope: &OperationScope,
    content: &[u8],
    _extent: crate::InputExtent,
    limits: ManagedStagingLimits,
) -> MimeResult<ResourceRef<StagedMimeInput>> {
    let size = u64::try_from(content.len()).map_err(|_| MimeError::BudgetExceeded {
        kind: "staging",
        requested: u64::MAX,
        limit: limits.max_bytes,
    })?;
    if !within_staging_limit(size, limits.max_bytes) {
        return Err(MimeError::BudgetExceeded {
            kind: "staging",
            requested: size,
            limit: limits.max_bytes,
        });
    }
    ensure_not_stopped(scope)?;
    let reservation = scope
        .reserve_resource()
        .map_err(|_| MimeError::ExecutionServicesRequired)?;
    let staged = create_empty_staged(scope, reservation, &limits).await?;
    for bytes in content.chunks(8 * 1024) {
        ensure_not_stopped(scope)?;
        let chunk_size = u64::try_from(bytes.len()).map_err(|_| MimeError::BudgetExceeded {
            kind: "staging",
            requested: u64::MAX,
            limit: limits.max_bytes,
        })?;
        let staging_permit = limits.budget.try_reserve(chunk_size).ok_or(MimeError::BudgetExceeded {
            kind: "staging",
            requested: chunk_size,
            limit: limits.max_bytes,
        })?;
        let owned_permit = limits
            .budget
            .try_reserve_owned(bytes.len())
            .ok_or(MimeError::BudgetExceeded {
                kind: "owned",
                requested: bytes.len() as u64,
                limit: 16 * 1024 * 1024,
            })?;
        write_chunk_in(
            scope,
            &limits.worker,
            staged.clone(),
            bytes.to_vec(),
            staging_permit,
            owned_permit,
        )
        .await?;
    }
    Ok(staged)
}

/// Reads one provider object and stages it without reopening its path.
///
/// Reads are charged to the shared tree budget, and a one-byte sentinel is
/// used when needed to prove the per-request staging limit was not exceeded.
///
/// # Parameters
/// - `scope`: Operation scope that owns the staged resource.
/// - `file_system`: Provider filesystem used to open the input once.
/// - `path`: Provider path to read.
/// - `limits`: Per-request staging and execution limits.
/// - `read_tree`: Shared provider-read budget context.
///
/// # Returns
/// The registered staged-file resource.
///
/// # Errors
/// Returns an error when opening, reading, dispatch, or a budget check fails.
///
/// # Parameters
/// - `scope`: Operation scope that owns the staged resource.
/// - `file_system`: Provider filesystem used to open the input once.
/// - `path`: Provider path to read.
/// - `limits`: Per-request staging and execution limits.
/// - `read_tree`: Shared provider-read budget context.
///
/// # Returns
/// The registered staged-file resource.
///
/// # Errors
/// Returns an error when opening, reading, dispatch, or a budget check fails.
async fn stage_provider_in(
    scope: &OperationScope,
    file_system: Arc<AsyncFileSystem>,
    path: FsPath,
    limits: ManagedStagingLimits,
    read_tree: Arc<ReadTreeContext>,
) -> MimeResult<ResourceRef<StagedMimeInput>> {
    ensure_not_stopped(scope)?;

    // Exactly one open binds every sequential read to the same provider
    // object. No metadata/stat call or path-based reopen is used.
    let mut reader = file_system
        .open_reader(&path, ReadOptions::default())
        .await
        .map_err(MimeError::FileSystem)?;
    ensure_not_stopped(scope)?;
    let reservation = scope
        .reserve_resource()
        .map_err(|_| MimeError::ExecutionServicesRequired)?;
    ensure_not_stopped(scope)?;
    let staged = create_empty_staged(scope, reservation, &limits).await?;
    let _chunk_buffer_permit = limits
        .budget
        .try_reserve_owned(8 * 1024)
        .ok_or(MimeError::BudgetExceeded {
            kind: "owned",
            requested: 8 * 1024,
            limit: 16 * 1024 * 1024,
        })?;
    let mut total = 0_u64;
    let mut chunk = vec![0_u8; 8 * 1024];
    loop {
        ensure_not_stopped(scope)?;
        let staging_remaining = limits.max_bytes.saturating_sub(total);
        // Read no more than the remaining staged allowance. Once it reaches
        // zero, reserve exactly one additional byte to prove EOF; that byte
        // is charged to the shared read tree but never written to disk.
        let staging_request = if staging_remaining == 0 {
            1
        } else {
            staging_remaining.min(chunk.len() as u64)
        };
        let tree_remaining = read_tree.remaining();
        if tree_remaining == 0 {
            return Err(MimeError::BudgetExceeded {
                kind: "read",
                requested: staging_request,
                limit: 0,
            });
        }
        let ask_u64 = staging_request.min(tree_remaining);
        let permit: ReadPermit = read_tree.reserve(ask_u64).ok_or(MimeError::BudgetExceeded {
            kind: "read",
            requested: staging_request,
            limit: tree_remaining,
        })?;
        let ask = usize::try_from(ask_u64).expect("request is capped by fixed chunk size");
        ensure_not_stopped(scope)?;
        let count = reader.read_async(&mut chunk[..ask]).await.map_err(|error| {
            // A single AsyncInput read either transfers its returned count or
            // transfers no bytes on error, so dropping the permit rolls back
            // the reservation safely.
            MimeError::Io(error)
        })?;
        let count_u64 = u64::try_from(count).map_err(|_| MimeError::BudgetExceeded {
            kind: "read",
            requested: u64::MAX,
            limit: ask_u64,
        })?;
        permit.commit(count_u64);
        ensure_not_stopped(scope)?;
        if count == 0 {
            return Ok(staged);
        }
        let new_total = total.checked_add(count_u64).ok_or(MimeError::BudgetExceeded {
            kind: "staging",
            requested: u64::MAX,
            limit: limits.max_bytes,
        })?;
        if new_total > limits.max_bytes {
            return Err(MimeError::BudgetExceeded {
                kind: "staging",
                requested: new_total,
                limit: limits.max_bytes,
            });
        }
        let permit = limits.budget.try_reserve(count_u64).ok_or(MimeError::BudgetExceeded {
            kind: "staging",
            requested: new_total,
            limit: limits.max_bytes,
        })?;
        let owned_permit = limits
            .budget
            .try_reserve_owned(count)
            .ok_or(MimeError::BudgetExceeded {
                kind: "owned",
                requested: count as u64,
                limit: 16 * 1024 * 1024,
            })?;
        let bytes = chunk[..count].to_vec();
        write_chunk_in(scope, &limits.worker, staged.clone(), bytes, permit, owned_permit).await?;
        total = new_total;
    }
}

/// Creates a temporary-file owner on the MIME execution worker and registers
/// it with the supplied operation reservation before returning a resource
/// reference.
///
/// # Parameters
/// - `scope`: Operation scope used to register and own the resource.
/// - `reservation`: Resource reservation committed after file creation.
/// - `limits`: Worker and budget settings for this request.
///
/// # Returns
/// The registered staged-file resource.
///
/// # Errors
/// Returns an error when the operation stops or the worker rejects creation.
///
/// # Parameters
/// - `scope`: Operation scope used to register and own the resource.
/// - `reservation`: Resource reservation committed after file creation.
/// - `limits`: Worker and budget settings for this request.
///
/// # Returns
/// The registered staged-file resource.
///
/// # Errors
/// Returns an error when the operation stops or the worker rejects creation.
async fn create_empty_staged(
    scope: &OperationScope,
    reservation: ResourceReservation,
    limits: &ManagedStagingLimits,
) -> MimeResult<ResourceRef<StagedMimeInput>> {
    ensure_not_stopped(scope)?;
    let context = scope.context().clone();
    let (sender, receiver) = completion_pair();
    let worker = limits.worker.clone();
    let job = Box::new(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if let Some(reason) = context.stop_reason() {
                return Err(stopped_error(reason));
            }
            let fs = LocalFileSystem::host().map_err(|e| MimeError::Io(e.into_io_error()))?;
            let temp = fs.create_temp_file().map_err(|e| MimeError::Io(e.into_io_error()))?;
            let staged = StagedMimeInput {
                owner: Mutex::new(temp),
                permits: Mutex::new(Vec::new()),
                worker: worker.clone(),
                cleanup_id: Arc::new(Mutex::new(None)),
            };
            commit_staged(reservation, staged)
        }))
        .unwrap_or_else(|payload| {
            Err(MimeError::detector_backend(
                "managed staging",
                panic_message(payload.as_ref()),
            ))
        });
        sender.complete(result);
    });
    if let Err(job) = limits.worker.try_dispatch(job) {
        drop(job);
        return Err(MimeError::ExecutionServicesRequired);
    }
    receiver.await
}

/// Writes one already-budgeted chunk on the MIME execution worker.
///
/// The closure owns the bytes and permits until the write finishes, including
/// when dispatch is rejected or the operation stops.
///
/// # Parameters
/// - `scope`: Operation scope checked before dispatch.
/// - `worker`: Worker that performs the write.
/// - `staged`: Registered staged-file resource.
/// - `bytes`: Chunk to write.
/// - `permit`: Staging budget held for the chunk.
/// - `owned_permit`: Temporary memory budget held for the chunk.
///
/// # Returns
/// `Ok(())` after the chunk is written.
///
/// # Errors
/// Returns an error when stopping, dispatch, or the file write fails.
///
/// # Parameters
/// - `scope`: Operation scope checked before dispatch.
/// - `worker`: Worker that performs the write.
/// - `staged`: Registered staged-file resource.
/// - `bytes`: Chunk to write.
/// - `permit`: Staging budget held for the chunk.
/// - `owned_permit`: Temporary memory budget held for the chunk.
///
/// # Returns
/// `Ok(())` after the chunk is written.
///
/// # Errors
/// Returns an error when stopping, dispatch, or the file write fails.
async fn write_chunk_in(
    scope: &OperationScope,
    worker: &MimeExecutionWorker,
    staged: ResourceRef<StagedMimeInput>,
    bytes: Vec<u8>,
    permit: StagingPermit,
    owned_permit: OwnedMemoryPermit,
) -> MimeResult<()> {
    ensure_not_stopped(scope)?;
    let context = scope.context().clone();
    let (sender, receiver) = completion_pair();
    let job = Box::new(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let result = if let Some(reason) = context.stop_reason() {
                Err(stopped_error(reason))
            } else {
                staged.write_chunk(&bytes, permit)
            };
            drop(bytes);
            drop(owned_permit);
            result
        }))
        .unwrap_or_else(|payload| {
            Err(MimeError::detector_backend(
                "managed staging",
                panic_message(payload.as_ref()),
            ))
        });
        sender.complete(result);
    });
    if let Err(job) = worker.try_dispatch(job) {
        drop(job);
        return Err(MimeError::ExecutionServicesRequired);
    }
    receiver.await
}

/// Commits a staged file and records a cleanup owner for later settlement.
///
/// # Parameters
/// - `reservation`: Operation reservation that will own the staged file.
/// - `staged`: Temporary-file resource and its associated budget permits.
///
/// # Returns
/// A reference to the committed resource, or an error if commit cannot finish.
///
/// # Errors
/// On commit failure the reservation retains the partial resource for cleanup
/// before this method returns an error.
pub(crate) fn commit_staged(
    reservation: ResourceReservation,
    staged: StagedMimeInput,
) -> MimeResult<ResourceRef<StagedMimeInput>> {
    let cleanup_id = Arc::clone(&staged.cleanup_id);
    let policy = Arc::new(ManagedTempCleanup {
        resource_id: Arc::clone(&cleanup_id),
    });
    match reservation.commit_with(staged, Arc::clone(&policy)) {
        Ok(resource) => {
            *cleanup_id.lock().unwrap_or_else(|poison| poison.into_inner()) = Some(resource.id());
            Ok(resource)
        }
        Err(error) => {
            let (_, partial, cleanup, reservation) = error.into_parts();
            let id = reservation.retain_with_for_cleanup(partial, cleanup);
            *cleanup_id.lock().unwrap_or_else(|poison| poison.into_inner()) = Some(id);
            Err(MimeError::ExecutionServicesRequired)
        }
    }
}

/// Returns whether a staged object fits within its byte limit.
///
/// # Parameters
/// - `size`: Requested staged byte count.
/// - `limit`: Maximum permitted staged byte count.
///
/// # Returns
/// `true` when `size` does not exceed `limit`.
/// Returns whether a staged object fits within its byte limit.
///
/// # Parameters
/// - `size`: Requested staged byte count.
/// - `limit`: Maximum permitted staged byte count.
///
/// # Returns
/// `true` when `size` does not exceed `limit`.
pub(crate) const fn within_staging_limit(size: u64, limit: u64) -> bool {
    size <= limit
}

/// Returns the optional one-byte sentinel budget without overflowing at the
/// integer boundary.
#[cfg(test)]
pub(crate) const fn sentinel_read_limit(limit: u64) -> Option<u64> {
    limit.checked_add(1)
}

/// Extracts a readable message from a worker panic payload.
///
/// # Parameters
/// - `payload`: Panic payload captured from a worker callback.
///
/// # Returns
/// The panic string, or a fallback message when it is not a string payload.
fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&'static str>().map(|text| (*text).to_owned()))
        .unwrap_or_else(|| "temporary cleanup panicked".to_owned())
}

/// Builds a diagnostic that identifies the registered temp resource and
/// preserves the underlying cleanup error as its source.
///
/// # Parameters
/// - `id`: Shared identity of the registered staged resource.
/// - `source`: Cleanup error retained as the diagnostic source.
///
/// # Returns
/// A cleanup failure associated with the staged resource.
fn cleanup_failure(
    id: &Arc<Mutex<Option<ResourceId>>>,
    source: Arc<dyn std::error::Error + Send + Sync>,
) -> CleanupFailure {
    CleanupFailure {
        resource_id: id
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .expect("registered staging resource has a real operation identity"),
        stage: CleanupStage::Drop,
        summary: DiagnosticSummary::new(&source.to_string()),
        source: Some(source),
    }
}
