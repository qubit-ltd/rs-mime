// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Async cleanup owner for staged temporary files.

use std::sync::Arc;
use std::sync::Mutex;

use qubit_operation::AsyncCleanup;
use qubit_operation::CleanupContext;
use qubit_operation::CleanupFuture;
use qubit_operation::CleanupOutcome;
use qubit_operation::ResourceId;

use super::StagedMimeInput;
use super::cleanup_failure;
use super::completion::completion_pair;
use super::panic_message;

/// Cleanup adapter that dispatches staged-file deletion to the MIME worker.
pub(super) struct ManagedTempCleanup {
    /// Resource ID recorded for cleanup failure diagnostics.
    pub(super) resource_id: Arc<Mutex<Option<ResourceId>>>,
}
impl AsyncCleanup<StagedMimeInput> for ManagedTempCleanup {
    /// Removes the staged file and returns either successful cleanup or the
    /// original resource for explicit recovery.
    ///
    /// # Parameters
    /// - `self`: Shared cleanup adapter.
    /// - `resource`: Staged file whose owner is being removed.
    /// - `_context`: Cleanup context supplied by the operation manager.
    ///
    /// # Returns
    /// A future resolving to released cleanup or a recoverable failure.
    fn cleanup(self: Arc<Self>, resource: StagedMimeInput, _context: CleanupContext) -> CleanupFuture<StagedMimeInput> {
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
                    CleanupOutcome::Released
                }
                Ok(Err(error)) => CleanupOutcome::Failed {
                    failure: cleanup_failure(&cleanup_id, Arc::new(error)),
                    resource,
                },
                Err(payload) => CleanupOutcome::Failed {
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
            return Box::pin(async move { CleanupOutcome::Failed { failure, resource } });
        }
        Box::pin(async move { receiver.await })
    }
}
