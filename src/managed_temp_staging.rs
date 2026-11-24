// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

//! Managed temporary-file staging for MIME command inputs.
//!
//! `max_buffer_bytes` limits the input payload retained by one request; it
//! does not size these streaming work buffers. Provider staging uses an 8 KiB
//! read buffer, while byte-slice staging copies at most 8 KiB per chunk. The
//! buffer and each live chunk copy reserve owned memory against the shared
//! service budget. Request staging limits count bytes written to the staged
//! file, and the provider path may read one additional byte to prove that the
//! staging limit was not exceeded.

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
use crate::detector::MimeExecutionDispatchFailureKind;
use crate::detector::OwnedMemoryPermit;
use crate::detector::ReadPermit;
use crate::detector::ReadTreeContext;
use crate::detector::StagingPermit;
use crate::detector::mime_input::OwnedMimeInput;

mod completion;
mod managed_staging_limits;
mod managed_temp_cleanup;
mod resource_identity;
mod staged_mime_input;
mod staged_temp_owner;

use completion::completion_pair;
pub(crate) use managed_staging_limits::ManagedStagingLimits;
use managed_temp_cleanup::ManagedTempCleanup;
use resource_identity::ResourceIdentity;
pub(crate) use staged_mime_input::StagedMimeInput;
use staged_temp_owner::StagedTempOwner;

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
fn ensure_not_stopped(scope: &OperationScope) -> MimeResult<()> {
    if let Some(reason) = scope.context().stop_reason() {
        return Err(stopped_error(reason));
    }
    if scope.context().is_expired() {
        return Err(stopped_error(StopReason::Timeout));
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
fn stopped_error(reason: StopReason) -> MimeError {
    MimeError::OperationStopped(Box::new(StopReport {
        reason,
        accepted_at: None,
    }))
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
/// Complete staging requires remaining read allowance for a read that returns
/// EOF; reaching the byte limit alone does not prove that the object ended.
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
async fn create_empty_staged(
    scope: &OperationScope,
    reservation: ResourceReservation,
    limits: &ManagedStagingLimits,
) -> MimeResult<ResourceRef<StagedMimeInput>> {
    create_empty_staged_with(scope, reservation, limits, || {
        LocalFileSystem::host()?.create_temp_file().map_err(Box::new)
    })
    .await
}

/// Creates and registers a staged file using an injected native creation step.
///
/// The seam keeps native creation on the execution worker while allowing
/// deterministic tests of filesystem failures that carry recovery ownership.
///
/// # Parameters
/// - `scope`: Operation scope that owns the resulting staged resource.
/// - `reservation`: Reserved resource slot consumed by successful creation.
/// - `limits`: Per-request and service-wide staging limits.
/// - `create_temp`: Native temporary-file creation operation dispatched to the
///   worker.
///
/// # Returns
/// A reference to the registered staged file resource.
///
/// # Errors
/// Returns an error when creation, registration, or operation state rejects
/// staging.
pub(crate) async fn create_empty_staged_with(
    scope: &OperationScope,
    reservation: ResourceReservation,
    limits: &ManagedStagingLimits,
    create_temp: impl FnOnce() -> Result<qubit_local_files::LocalTempFile, Box<qubit_local_files::LocalFileError>>
    + Send
    + 'static,
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
            match create_temp() {
                Ok(temp) => {
                    let staged = StagedMimeInput {
                        owner: Mutex::new(StagedTempOwner::File { owner: temp }),
                        permits: Mutex::new(Vec::new()),
                        worker: worker.clone(),
                        staged_bytes: std::sync::atomic::AtomicU64::new(0),
                        cleanup_id: Arc::new(ResourceIdentity::default()),
                    };
                    commit_staged(reservation, staged)
                }
                Err(error) => {
                    let (primary, recovery) = (*error).into_temp_recovery_parts();
                    if let Some(recovery) = recovery {
                        let staged = StagedMimeInput::from_recovery(recovery, worker.clone());
                        retain_creation_recovery(reservation, staged);
                    }
                    Err(MimeError::Io(primary.into_io_error()))
                }
            }
        }))
        .unwrap_or_else(|payload| {
            Err(MimeError::detector_backend(
                "managed staging",
                panic_message(payload.as_ref()),
            ))
        });
        sender.complete(result);
    });
    if let Err(failure) = limits.worker.try_dispatch_with_status(job) {
        let reason = failure.kind();
        drop(failure.into_job());
        return Err(match reason {
            MimeExecutionDispatchFailureKind::QueueFull => MimeError::CommandQueueBusy,
            MimeExecutionDispatchFailureKind::Unavailable => MimeError::ExecutionServicesRequired,
        });
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
    if let Err(failure) = worker.try_dispatch_with_status(job) {
        let reason = failure.kind();
        drop(failure.into_job());
        return Err(match reason {
            MimeExecutionDispatchFailureKind::QueueFull => MimeError::CommandQueueBusy,
            MimeExecutionDispatchFailureKind::Unavailable => MimeError::ExecutionServicesRequired,
        });
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
        attempts: std::sync::atomic::AtomicUsize::new(0),
    });
    match reservation.commit_with(staged, Arc::clone(&policy)) {
        Ok(resource) => {
            cleanup_id.publish(resource.id());
            Ok(resource)
        }
        Err(error) => {
            let (_, partial, cleanup, reservation) = error.into_parts();
            retain_partial_for_cleanup(reservation, partial, cleanup);
            Err(MimeError::ExecutionServicesRequired)
        }
    }
}

fn retain_creation_recovery(reservation: ResourceReservation, staged: StagedMimeInput) {
    let identity = Arc::clone(&staged.cleanup_id);
    let cleanup = Arc::new(ManagedTempCleanup {
        resource_id: Arc::clone(&identity),
        attempts: std::sync::atomic::AtomicUsize::new(0),
    });
    let id = reservation.retain_with_for_cleanup(staged, cleanup);
    identity.publish(id);
}

fn retain_partial_for_cleanup(
    reservation: ResourceReservation,
    partial: StagedMimeInput,
    cleanup: Arc<ManagedTempCleanup>,
) {
    let identity = Arc::clone(&partial.cleanup_id);
    let id = reservation.retain_with_for_cleanup(partial, cleanup);
    identity.publish(id);
}

/// Returns whether a staged object fits within its byte limit.
///
/// # Parameters
/// - `size`: Requested staged byte count.
/// - `limit`: Maximum permitted staged byte count.
///
/// # Returns
/// `true` when `size` does not exceed `limit`.
#[must_use]
#[inline]
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
#[must_use]
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
    id: ResourceId,
    source: Arc<dyn std::error::Error + Send + Sync>,
) -> CleanupFailure {
    CleanupFailure {
        resource_id: id,
        stage: CleanupStage::Drop,
        summary: DiagnosticSummary::new(&source.to_string()),
        source: Some(source),
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use std::future::Future;
    use std::sync::Arc;
    use std::sync::mpsc;
    use std::task::Context;
    use std::task::Poll;
    use std::task::Wake;
    use std::task::Waker;

    use qubit_operation::GroupOptions;
    use qubit_operation::OperationManager;

    use super::ManagedStagingLimits;
    use super::create_empty_staged_with;
    use super::panic_message;
    use super::stage_borrowed_bytes_in;
    use super::stopped_error;
    use crate::InputExtent;
    use crate::command_execution::FixedMimeExecutionPort;
    use crate::command_execution::MimeExecutionWorker;
    use crate::detector::MimeExecutionPort;
    use crate::detector::MimeExecutionServices;

    struct ThreadWake(std::thread::Thread);

    impl Wake for ThreadWake {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }

    fn block_on<F: Future>(future: F) -> F::Output {
        let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
        let mut context = Context::from_waker(&waker);
        let mut future = std::pin::pin!(future);
        loop {
            match future.as_mut().poll(&mut context) {
                Poll::Ready(value) => return value,
                Poll::Pending => std::thread::park(),
            }
        }
    }

    #[test]
    fn test_worker_panic_payloads_become_readable_diagnostics() {
        let owned_string = String::from("owned worker panic");
        assert_eq!("owned worker panic", panic_message(&owned_string));
        assert_eq!("static worker panic", panic_message(&"static worker panic"));
        assert_eq!("temporary cleanup panicked", panic_message(&42_u8));
    }

    #[test]
    fn test_staging_stop_error_preserves_reason_without_acceptance_time() {
        let error = stopped_error(qubit_operation::StopReason::UserCancelled);

        assert!(matches!(
            error,
            crate::MimeError::OperationStopped(ref report)
                if report.reason == qubit_operation::StopReason::UserCancelled
                    && report.accepted_at.is_none()
        ));
    }

    #[test]
    fn test_temp_creation_worker_panic_returns_error_and_releases_reservation() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 1 }).unwrap();
        let port = Arc::new(FixedMimeExecutionPort::new(1, 1));
        let services = Arc::new(MimeExecutionServices::new(manager, group.clone(), port.clone(), 64).unwrap());
        services.prepare().unwrap();
        let limits = ManagedStagingLimits {
            max_bytes: 8,
            budget: services.staging_budget(),
            worker: MimeExecutionWorker::new(port),
        };
        let mut operation = block_on(group.try_submit_async(Default::default(), move |scope| async move {
            let reservation = scope.reserve_resource().unwrap();
            let result = create_empty_staged_with(&scope, reservation, &limits, || {
                panic!("injected temporary creation panic")
            })
            .await;
            Ok::<_, crate::MimeError>(matches!(
                result,
                Err(crate::MimeError::DetectorBackend { ref backend, ref reason })
                    if backend == "managed staging" && reason == "injected temporary creation panic"
            ))
        }))
        .unwrap();

        assert!(matches!(
            block_on(operation.wait_outcome()).unwrap(),
            qubit_operation::OperationOutcome::Succeeded(true)
        ));
        assert!(block_on(operation.wait_settlement()).all_released());
        assert_eq!(0, services.staging_budget().used_bytes());
        let shutdown = services.shutdown();
        let _ = block_on(shutdown.wait_services_settled()).unwrap();
        let _ = shutdown.stop_services().unwrap();
        let _report = block_on(shutdown.wait_group_shutdown()).unwrap();
    }

    #[test]
    fn test_borrowed_staging_copies_multiple_chunks_and_releases_budget_on_cleanup() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 2 }).unwrap();
        let port = Arc::new(FixedMimeExecutionPort::new(1, 2));
        let services = Arc::new(MimeExecutionServices::new(manager, group.clone(), port.clone(), 64).unwrap());
        services.prepare().unwrap();

        let content = vec![b'b'; 8 * 1024 + 17];
        let max_bytes = content.len() as u64;
        let body_content = content.clone();
        let limits = ManagedStagingLimits {
            max_bytes,
            budget: services.staging_budget(),
            worker: MimeExecutionWorker::new(port.clone()),
        };
        let mut operation = block_on(group.try_submit_async(Default::default(), move |scope| async move {
            stage_borrowed_bytes_in(&scope, &body_content, InputExtent::Complete, limits).await
        }))
        .unwrap();

        let resource = match block_on(operation.wait_outcome()).unwrap() {
            qubit_operation::OperationOutcome::Succeeded(resource) => resource,
            _ => panic!("borrowed byte staging should succeed"),
        };
        let path = resource.path().unwrap();
        assert_eq!(content, std::fs::read(&path).unwrap());
        assert_eq!(max_bytes, services.staging_budget().used_bytes());

        drop(resource);
        assert!(block_on(operation.wait_settlement()).all_released());
        assert!(!path.exists(), "the registered owner is deleted after settlement");
        assert_eq!(0, services.staging_budget().used_bytes());

        let shutdown = services.shutdown();
        let _ = block_on(shutdown.wait_services_settled()).unwrap();
        let _ = shutdown.stop_services().unwrap();
        let _report = block_on(shutdown.wait_group_shutdown()).unwrap();
    }

    #[test]
    fn test_empty_borrowed_input_creates_and_releases_an_empty_staged_file() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 2 }).unwrap();
        let port = Arc::new(FixedMimeExecutionPort::new(1, 2));
        let services = Arc::new(MimeExecutionServices::new(manager, group.clone(), port.clone(), 64).unwrap());
        services.prepare().unwrap();
        let limits = ManagedStagingLimits {
            max_bytes: 0,
            budget: services.staging_budget(),
            worker: MimeExecutionWorker::new(port),
        };

        let mut operation = block_on(group.try_submit_async(Default::default(), move |scope| async move {
            stage_borrowed_bytes_in(&scope, &[], InputExtent::Complete, limits).await
        }))
        .unwrap();
        let resource = match block_on(operation.wait_outcome()).unwrap() {
            qubit_operation::OperationOutcome::Succeeded(resource) => resource,
            _ => panic!("empty input should still stage as an empty file"),
        };
        let path = resource.path().unwrap();
        assert!(path.exists());
        assert!(std::fs::read(&path).unwrap().is_empty());
        assert_eq!(0, services.staging_budget().used_bytes());

        drop(resource);
        assert!(block_on(operation.wait_settlement()).all_released());
        assert!(!path.exists());
        let shutdown = services.shutdown();
        let _ = block_on(shutdown.wait_services_settled()).unwrap();
        let _ = shutdown.stop_services().unwrap();
        let _report = block_on(shutdown.wait_group_shutdown()).unwrap();
    }

    #[test]
    fn test_borrowed_input_over_staging_limit_fails_before_resource_creation() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 2 }).unwrap();
        let port = Arc::new(FixedMimeExecutionPort::new(1, 2));
        let services = Arc::new(MimeExecutionServices::new(manager, group.clone(), port.clone(), 64).unwrap());
        services.prepare().unwrap();
        let limits = ManagedStagingLimits {
            max_bytes: 3,
            budget: services.staging_budget(),
            worker: MimeExecutionWorker::new(port),
        };
        let content = b"four".to_vec();
        let mut operation = block_on(group.try_submit_async(Default::default(), move |scope| async move {
            stage_borrowed_bytes_in(&scope, &content, InputExtent::Complete, limits).await
        }))
        .unwrap();

        assert!(matches!(
            block_on(operation.wait_outcome()).unwrap(),
            qubit_operation::OperationOutcome::Failed(crate::MimeError::BudgetExceeded {
                kind: "staging",
                requested: 4,
                limit: 3,
            })
        ));
        assert!(block_on(operation.wait_settlement()).all_released());
        assert_eq!(0, services.staging_budget().used_bytes());

        let shutdown = services.shutdown();
        let _ = block_on(shutdown.wait_services_settled()).unwrap();
        let _ = shutdown.stop_services().unwrap();
        let _report = block_on(shutdown.wait_group_shutdown()).unwrap();
    }

    #[test]
    fn test_chunk_budget_failure_after_file_creation_is_cleaned_during_settlement() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 2 }).unwrap();
        let port = Arc::new(FixedMimeExecutionPort::new(1, 2));
        let services = Arc::new(MimeExecutionServices::new(manager, group.clone(), port.clone(), 16).unwrap());
        services.prepare().unwrap();
        let budget = services.staging_budget();
        let occupied = budget.try_reserve(256 * 1024 * 1024 - 1).unwrap();
        let limits = ManagedStagingLimits {
            max_bytes: 2,
            budget: budget.clone(),
            worker: MimeExecutionWorker::new(port),
        };
        let mut operation = block_on(group.try_submit_async(Default::default(), move |scope| async move {
            stage_borrowed_bytes_in(&scope, b"xy", InputExtent::Complete, limits).await
        }))
        .unwrap();

        assert!(matches!(
            block_on(operation.wait_outcome()).unwrap(),
            qubit_operation::OperationOutcome::Failed(crate::MimeError::BudgetExceeded {
                kind: "staging",
                requested: 2,
                limit: 2,
            })
        ));
        assert!(block_on(operation.wait_settlement()).all_released());
        drop(occupied);
        assert_eq!(0, budget.used_bytes(), "failed chunk and occupied permits are released");

        let shutdown = services.shutdown();
        let _ = block_on(shutdown.wait_services_settled()).unwrap();
        let _ = shutdown.stop_services().unwrap();
        let _report = block_on(shutdown.wait_group_shutdown()).unwrap();
    }

    #[test]
    fn test_unprepared_worker_rejection_releases_resource_reservation() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 1 }).unwrap();
        let port = Arc::new(FixedMimeExecutionPort::new(1, 1));
        let services = MimeExecutionServices::new(manager, group.clone(), port.clone(), 16).unwrap();
        let budget = services.staging_budget();
        let limits = ManagedStagingLimits {
            max_bytes: 16,
            budget: budget.clone(),
            worker: MimeExecutionWorker::new(port),
        };
        let mut operation = block_on(group.try_submit_async(Default::default(), move |scope| async move {
            stage_borrowed_bytes_in(&scope, b"queued nowhere", InputExtent::Complete, limits).await
        }))
        .unwrap();

        assert!(matches!(
            block_on(operation.wait_outcome()).unwrap(),
            qubit_operation::OperationOutcome::Failed(crate::MimeError::ExecutionServicesRequired)
        ));
        assert!(block_on(operation.wait_settlement()).all_released());
        assert_eq!(0, budget.used_bytes());
    }

    #[test]
    fn test_cancelled_scope_skips_staging_before_reserving_a_resource() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 1 }).unwrap();
        let port = Arc::new(FixedMimeExecutionPort::new(1, 1));
        let services = Arc::new(MimeExecutionServices::new(manager, group.clone(), port.clone(), 16).unwrap());
        services.prepare().unwrap();
        let budget = services.staging_budget();
        let limits = ManagedStagingLimits {
            max_bytes: 16,
            budget: budget.clone(),
            worker: MimeExecutionWorker::new(port),
        };
        let (entered_tx, entered_rx) = mpsc::channel();
        let (continue_tx, continue_rx) = mpsc::channel();
        let mut operation = block_on(group.try_submit_async(Default::default(), move |scope| async move {
            entered_tx.send(()).unwrap();
            continue_rx.recv().unwrap();
            stage_borrowed_bytes_in(&scope, b"cancel before stage", InputExtent::Complete, limits).await
        }))
        .unwrap();

        entered_rx.recv().unwrap();
        let _stop_report = operation.request_stop(qubit_operation::StopReason::UserCancelled);
        continue_tx.send(()).unwrap();
        assert!(matches!(
            block_on(operation.wait_outcome()).unwrap(),
            qubit_operation::OperationOutcome::Stopped(ref report)
                if report.reason == qubit_operation::StopReason::UserCancelled
        ));
        assert!(block_on(operation.wait_settlement()).all_released());
        assert_eq!(
            0,
            budget.used_bytes(),
            "no staging budget is held when cancellation wins first"
        );

        let shutdown = services.shutdown();
        let _ = block_on(shutdown.wait_services_settled()).unwrap();
        let _ = shutdown.stop_services().unwrap();
        let _report = block_on(shutdown.wait_group_shutdown()).unwrap();
    }

    #[derive(Debug)]
    struct DispatchObservedPort {
        inner: Arc<FixedMimeExecutionPort>,
        dispatch_started: std::sync::Mutex<Option<mpsc::Sender<()>>>,
        job_finished: std::sync::Mutex<Option<mpsc::Sender<()>>>,
    }

    impl MimeExecutionPort for DispatchObservedPort {
        fn prepare(&self) -> Result<(), crate::MimeError> {
            self.inner.prepare()
        }

        fn try_dispatch(
            &self,
            job: Box<dyn FnOnce() + Send + 'static>,
        ) -> Result<(), Box<dyn FnOnce() + Send + 'static>> {
            if let Some(sender) = self.dispatch_started.lock().unwrap().take() {
                let _ = sender.send(());
            }
            let finished = self.job_finished.lock().unwrap().take();
            self.inner.try_dispatch(Box::new(move || {
                job();
                if let Some(sender) = finished {
                    let _ = sender.send(());
                }
            }))
        }

        fn shutdown(&self) -> Result<(), crate::MimeError> {
            self.inner.shutdown()
        }
    }

    #[test]
    fn test_stop_after_create_dispatch_prevents_owner_creation() {
        let manager = OperationManager::new(Default::default()).unwrap();
        let group = manager.group(GroupOptions { capacity: 2 }).unwrap();
        let inner = Arc::new(FixedMimeExecutionPort::new(1, 2));
        let (dispatch_tx, dispatch_rx) = mpsc::channel();
        let (job_finished_tx, job_finished_rx) = mpsc::channel();
        let port = Arc::new(DispatchObservedPort {
            inner: Arc::clone(&inner),
            dispatch_started: std::sync::Mutex::new(Some(dispatch_tx)),
            job_finished: std::sync::Mutex::new(Some(job_finished_tx)),
        });
        let services = Arc::new(MimeExecutionServices::new(manager, group.clone(), port.clone(), 64).unwrap());
        services.prepare().unwrap();

        let (blocker_started_tx, blocker_started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        assert!(
            inner
                .try_dispatch(Box::new(move || {
                    blocker_started_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                }))
                .is_ok()
        );
        blocker_started_rx.recv().unwrap();

        let limits = ManagedStagingLimits {
            max_bytes: 7,
            budget: services.staging_budget(),
            worker: MimeExecutionWorker::new(port),
        };
        let (body_started_tx, body_started_rx) = mpsc::channel();
        let mut operation = block_on(group.try_submit_async(Default::default(), move |scope| async move {
            body_started_tx.send(()).unwrap();
            stage_borrowed_bytes_in(&scope, b"payload", InputExtent::Complete, limits).await
        }))
        .unwrap();

        body_started_rx.recv().unwrap();
        dispatch_rx.recv().unwrap();
        let _stop_report = operation.request_stop(qubit_operation::StopReason::UserCancelled);
        release_tx.send(()).unwrap();
        job_finished_rx.recv().unwrap();

        assert!(matches!(
            block_on(operation.wait_outcome()).unwrap(),
            qubit_operation::OperationOutcome::Stopped(ref report)
                if report.reason == qubit_operation::StopReason::UserCancelled
        ));
        assert!(block_on(operation.wait_settlement()).all_released());
        assert_eq!(0, services.staging_budget().used_bytes());

        let shutdown = services.shutdown();
        let _ = block_on(shutdown.wait_services_settled()).unwrap();
        let _ = shutdown.stop_services().unwrap();
        let _report = block_on(shutdown.wait_group_shutdown()).unwrap();
    }
}
