// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Managed temporary-file staging for MIME command inputs.

use std::sync::Arc;
use std::sync::Mutex;

use qubit_io::AsyncInput;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::LocalTempFile;
use qubit_operation::OperationScope;
use qubit_operation::ResourceId;
use qubit_operation::ResourceRef;

use crate::MimeError;
use crate::MimeFuture;
use crate::MimeResult;
use crate::command_execution::MimeExecutionWorker;
use crate::detector::OwnedMemoryPermit;
use crate::detector::ReadPermit;
use crate::detector::ReadTreeContext;
use crate::detector::StagingBudget;
use crate::detector::StagingPermit;
use crate::detector::mime_input::OwnedMimeInput;

/// Per-request staging cap and the shared service accounting handle.
#[derive(Clone)]
pub(crate) struct ManagedStagingLimits {
    pub(crate) max_bytes: u64,
    pub(crate) budget: StagingBudget,
    pub(crate) worker: MimeExecutionWorker,
}

/// Shared control port for a staged file. Its resource owner is registered
/// exactly once by operation; consumers receive only `ResourceRef`s.
pub(crate) struct StagedMimeInput {
    owner: Mutex<LocalTempFile>,
    permits: Mutex<Vec<StagingPermit>>,
    worker: MimeExecutionWorker,
    cleanup_id: Arc<Mutex<Option<ResourceId>>>,
}

impl StagedMimeInput {
    #[cfg(test)]
    pub(crate) fn new(owner: LocalTempFile, permit: StagingPermit, worker: MimeExecutionWorker) -> Self {
        Self {
            owner: Mutex::new(owner),
            permits: Mutex::new(vec![permit]),
            worker,
            cleanup_id: Arc::new(Mutex::new(None)),
        }
    }

    /// Returns a cloned native path for command argument construction.
    pub(crate) fn path(&self) -> MimeResult<std::path::PathBuf> {
        let owner = self.owner.lock().unwrap_or_else(|poison| poison.into_inner());
        Ok(owner.path().to_path_buf())
    }

    fn write_chunk(&self, bytes: &[u8], permit: StagingPermit) -> MimeResult<()> {
        // Store the permit before writing: if a partial write fails, the
        // registered resource continues to account for every reserved byte.
        self.permits.lock().unwrap_or_else(|p| p.into_inner()).push(permit);
        use std::io::Write;
        let mut owner = self.owner.lock().unwrap_or_else(|p| p.into_inner());
        owner
            .as_file_mut()
            .and_then(|file| file.write_all(bytes))
            .map_err(|error| MimeError::Io(error.into()))
    }
}

/// Stages owned bytes without performing filesystem work in the caller's
/// async poll. A rejected worker dispatch returns the job and drops the
/// reservation before any temporary file is created.
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

fn ensure_not_stopped(scope: &OperationScope) -> MimeResult<()> {
    if let Some(reason) = scope.context().stop_reason() {
        return Err(stopped_error(reason));
    }
    Ok(())
}

fn stopped_error(reason: qubit_operation::StopReason) -> MimeError {
    MimeError::OperationStopped(qubit_operation::StopReport {
        reason,
        accepted_at: None,
    })
}

pub(crate) fn stage_borrowed_bytes_in<'a>(
    scope: &'a OperationScope,
    content: &'a [u8],
    extent: crate::InputExtent,
    limits: ManagedStagingLimits,
) -> MimeFuture<'a, ResourceRef<StagedMimeInput>> {
    Box::pin(stage_byte_slice_in(scope, content, extent, limits))
}

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

async fn stage_provider_in(
    scope: &OperationScope,
    file_system: Arc<qubit_fs::AsyncFileSystem>,
    path: qubit_fs::Path,
    limits: ManagedStagingLimits,
    read_tree: Arc<ReadTreeContext>,
) -> MimeResult<ResourceRef<StagedMimeInput>> {
    use qubit_fs::read::ReadOptions;

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

async fn create_empty_staged(
    scope: &OperationScope,
    reservation: qubit_operation::ResourceReservation,
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

pub(crate) fn commit_staged(
    reservation: qubit_operation::ResourceReservation,
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

pub(crate) const fn within_staging_limit(size: u64, limit: u64) -> bool {
    size <= limit
}

/// Returns the optional one-byte sentinel budget without overflowing at the
/// integer boundary.
#[cfg(test)]
pub(crate) const fn sentinel_read_limit(limit: u64) -> Option<u64> {
    limit.checked_add(1)
}

struct ManagedTempCleanup {
    resource_id: Arc<Mutex<Option<ResourceId>>>,
}
impl qubit_operation::AsyncCleanup<StagedMimeInput> for ManagedTempCleanup {
    fn cleanup(
        self: Arc<Self>,
        resource: StagedMimeInput,
        _context: qubit_operation::CleanupContext,
    ) -> qubit_operation::CleanupFuture<StagedMimeInput> {
        let worker = resource.worker.clone();
        let cleanup_id = Arc::clone(&self.resource_id);
        let retained = Arc::new(Mutex::new(Some(resource)));
        let for_job = Arc::clone(&retained);
        let (sender, receiver) = completion_pair();
        let job = Box::new(move || {
            let Some(mut resource) = for_job.lock().unwrap_or_else(|poison| poison.into_inner()).take() else {
                return;
            };
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                resource
                    .owner
                    .get_mut()
                    .unwrap_or_else(|poison| poison.into_inner())
                    .cleanup()
            }));
            let outcome = match result {
                Ok(Ok(())) => {
                    // Return staged-byte capacity only after cleanup has
                    // confirmed the native owner is gone. Clearing explicitly
                    // makes budget release coincide with that evidence.
                    resource
                        .permits
                        .get_mut()
                        .unwrap_or_else(|poison| poison.into_inner())
                        .clear();
                    qubit_operation::CleanupOutcome::Released
                }
                Ok(Err(error)) => qubit_operation::CleanupOutcome::Failed {
                    failure: cleanup_failure(&cleanup_id, Arc::new(error)),
                    resource,
                },
                Err(payload) => qubit_operation::CleanupOutcome::Failed {
                    failure: cleanup_failure(
                        &cleanup_id,
                        Arc::new(std::io::Error::other(panic_message(payload.as_ref()))),
                    ),
                    resource,
                },
            };
            sender.complete(outcome);
        });
        if let Err(job) = worker.try_dispatch(job) {
            drop(job);
            let resource = retained
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .take()
                .expect("rejected cleanup job has not taken ownership");
            let failure = cleanup_failure(
                &resource.cleanup_id,
                Arc::new(std::io::Error::other("MIME worker rejected cleanup job")),
            );
            return Box::pin(async move { qubit_operation::CleanupOutcome::Failed { failure, resource } });
        }
        Box::pin(async move { receiver.await })
    }
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&'static str>().map(|text| (*text).to_owned()))
        .unwrap_or_else(|| "temporary cleanup panicked".to_owned())
}

fn cleanup_failure(
    id: &Arc<Mutex<Option<ResourceId>>>,
    source: Arc<dyn std::error::Error + Send + Sync>,
) -> qubit_operation::CleanupFailure {
    use qubit_operation::CleanupFailure;
    use qubit_operation::CleanupStage;
    use qubit_operation::DiagnosticSummary;
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

struct Completion<T> {
    value: Option<T>,
    waker: Option<std::task::Waker>,
}
struct Completer<T>(Arc<Mutex<Completion<T>>>);
struct CompletionFuture<T>(Arc<Mutex<Completion<T>>>);
fn completion_pair<T>() -> (Completer<T>, CompletionFuture<T>) {
    let state = Arc::new(Mutex::new(Completion {
        value: None,
        waker: None,
    }));
    (Completer(Arc::clone(&state)), CompletionFuture(state))
}
impl<T> Completer<T> {
    fn complete(self, value: T) {
        let mut state = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        state.value = Some(value);
        if let Some(waker) = state.waker.take() {
            waker.wake();
        }
    }
}
impl<T> std::future::Future for CompletionFuture<T> {
    type Output = T;
    fn poll(self: std::pin::Pin<&mut Self>, context: &mut std::task::Context<'_>) -> std::task::Poll<T> {
        let mut state = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        if let Some(value) = state.value.take() {
            std::task::Poll::Ready(value)
        } else {
            state.waker = Some(context.waker().clone());
            std::task::Poll::Pending
        }
    }
}
