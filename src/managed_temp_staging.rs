// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Managed temporary-file staging for MIME command inputs.

use std::sync::{Arc, Mutex};

use qubit_local_files::{LocalFileSystem, LocalTempFile};
use qubit_operation::{OperationScope, ResourceId, ResourceRef};

use crate::command_execution::MimeExecutionWorker;
use crate::detector::mime_input::OwnedMimeInput;
use crate::detector::{StagingBudget, StagingPermit};
use crate::{MimeError, MimeFuture, MimeResult};

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
    _permit: StagingPermit,
    worker: MimeExecutionWorker,
    cleanup_id: Arc<Mutex<Option<ResourceId>>>,
}

impl StagedMimeInput {
    pub(crate) fn new(
        owner: LocalTempFile,
        permit: StagingPermit,
        worker: MimeExecutionWorker,
    ) -> Self {
        Self {
            owner: Mutex::new(owner),
            _permit: permit,
            worker,
            cleanup_id: Arc::new(Mutex::new(None)),
        }
    }

    /// Returns a cloned native path for command argument construction.
    pub(crate) fn path(&self) -> MimeResult<std::path::PathBuf> {
        let owner = self
            .owner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        Ok(owner.path().to_path_buf())
    }
}

/// Stages owned bytes without performing filesystem work in the caller's
/// async poll. A rejected worker dispatch returns the job and drops the
/// reservation before any temporary file is created.
pub(crate) fn stage_owned_in<'a>(
    scope: &'a OperationScope,
    input: OwnedMimeInput,
    limits: ManagedStagingLimits,
) -> MimeFuture<'a, ResourceRef<StagedMimeInput>> {
    Box::pin(async move {
        let (content, extent) = match input {
            OwnedMimeInput::Bytes {
                content, extent, ..
            } => (content, extent),
            OwnedMimeInput::Provider { .. } => {
                return Err(MimeError::UnsupportedCapability {
                    capability: crate::MimeCapability::RandomAccessSampling,
                });
            }
        };
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
        if extent == crate::InputExtent::Prefix && content.is_empty() {
            // Empty prefixes are valid; this branch deliberately does not
            // infer Complete from length.
        }
        let permit = limits
            .budget
            .try_reserve(size)
            .ok_or(MimeError::BudgetExceeded {
                kind: "staging",
                requested: size,
                limit: limits.max_bytes,
            })?;
        let reservation = scope
            .reserve_resource()
            .map_err(|_| MimeError::ExecutionServicesRequired)?;
        let (sender, receiver) = completion_pair();
        let worker = limits.worker.clone();
        let job = Box::new(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                (|| {
                    let fs = LocalFileSystem::host()
                        .map_err(|error| MimeError::Io(error.into_io_error()))?;
                    let mut temp = fs
                        .create_temp_file()
                        .map_err(|error| MimeError::Io(error.into_io_error()))?;
                    use std::io::Write;
                    if let Err(error) = temp.as_file_mut().and_then(|file| file.write_all(&content))
                    {
                        let cleanup_id = Arc::new(Mutex::new(None));
                        let staged = StagedMimeInput {
                            owner: Mutex::new(temp),
                            _permit: permit,
                            worker: worker.clone(),
                            cleanup_id: Arc::clone(&cleanup_id),
                        };
                        let id = reservation.retain_with_for_cleanup(
                            staged,
                            Arc::new(ManagedTempCleanup {
                                resource_id: Arc::clone(&cleanup_id),
                            }),
                        );
                        *cleanup_id
                            .lock()
                            .unwrap_or_else(|poison| poison.into_inner()) = Some(id);
                        return Err(MimeError::Io(error));
                    }
                    commit_staged(
                        reservation,
                        StagedMimeInput::new(temp, permit, worker.clone()),
                    )
                })()
            }))
            .unwrap_or_else(|payload| {
                Err(MimeError::detector_backend(
                    "managed staging",
                    format!(
                        "staging worker panicked: {}",
                        panic_message(payload.as_ref())
                    ),
                ))
            });
            sender.complete(result);
        });
        if let Err(job) = limits.worker.try_dispatch(job) {
            drop(job);
            return Err(MimeError::ExecutionServicesRequired);
        }
        receiver.await
    })
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
            *cleanup_id
                .lock()
                .unwrap_or_else(|poison| poison.into_inner()) = Some(resource.id());
            Ok(resource)
        }
        Err(error) => {
            let (_, partial, cleanup, reservation) = error.into_parts();
            let id = reservation.retain_with_for_cleanup(partial, cleanup);
            *cleanup_id
                .lock()
                .unwrap_or_else(|poison| poison.into_inner()) = Some(id);
            Err(MimeError::ExecutionServicesRequired)
        }
    }
}

pub(crate) const fn within_staging_limit(size: u64, limit: u64) -> bool {
    size <= limit
}

/// Returns the optional one-byte sentinel budget without overflowing at the
/// integer boundary.
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
            let Some(mut resource) = for_job
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .take()
            else {
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
                Ok(Ok(())) => qubit_operation::CleanupOutcome::Released,
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
            return Box::pin(async move {
                qubit_operation::CleanupOutcome::Failed { failure, resource }
            });
        }
        Box::pin(async move { receiver.await })
    }
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| {
            payload
                .downcast_ref::<&'static str>()
                .map(|text| (*text).to_owned())
        })
        .unwrap_or_else(|| "temporary cleanup panicked".to_owned())
}

fn cleanup_failure(
    id: &Arc<Mutex<Option<ResourceId>>>,
    source: Arc<dyn std::error::Error + Send + Sync>,
) -> qubit_operation::CleanupFailure {
    use qubit_operation::{CleanupFailure, CleanupStage, DiagnosticSummary};
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
    fn poll(
        self: std::pin::Pin<&mut Self>,
        context: &mut std::task::Context<'_>,
    ) -> std::task::Poll<T> {
        let mut state = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        if let Some(value) = state.value.take() {
            std::task::Poll::Ready(value)
        } else {
            state.waker = Some(context.waker().clone());
            std::task::Poll::Pending
        }
    }
}
