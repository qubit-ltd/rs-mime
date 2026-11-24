// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Worker dispatch adapters for MIME command execution.

use std::sync::Arc;
use std::sync::Mutex;

use crate::detector::MimeExecutionDispatchFailureKind;
use crate::detector::MimeExecutionPort;

mod fixed_mime_execution_port;
mod fixed_queue;
mod queue_state;
mod running_workers;

pub use fixed_mime_execution_port::FixedMimeExecutionPort;

/// Cloneable handle to the service-owned fixed-capacity worker dispatcher.
#[derive(Clone)]
pub(crate) struct MimeExecutionWorker {
    /// Execution port shared with the application runtime.
    port: Arc<dyn MimeExecutionPort>,
}

impl MimeExecutionWorker {
    /// Uses the dispatch service already owned by the application.
    ///
    /// # Parameters
    /// - `port`: Application-owned execution port.
    #[must_use]
    pub(crate) fn new(port: Arc<dyn MimeExecutionPort>) -> Self {
        Self { port }
    }

    /// Test helper for controlled ports that need to enqueue raw jobs.
    #[cfg(test)]
    pub(crate) fn try_dispatch(
        &self,
        job: Box<dyn FnOnce() + Send + 'static>,
    ) -> Result<(), Box<dyn FnOnce() + Send + 'static>> {
        self.port.try_dispatch(job)
    }

    /// Dispatches a payload with a terminal action for accepted jobs that are
    /// explicitly destroyed before execution.
    pub(crate) fn try_dispatch_guarded<T, Run, Abandon>(
        &self,
        payload: T,
        run: Run,
        abandon: Abandon,
    ) -> Result<(), (MimeExecutionDispatchFailureKind, T)>
    where
        T: Send + 'static,
        Run: FnOnce(T) + Send + 'static,
        Abandon: FnOnce(T) + Send + 'static,
    {
        let state = Arc::new(Mutex::new(GuardState {
            accepted: false,
            rejected: false,
            started: false,
            dropped: false,
            payload: Some(payload),
            run: Some(run),
            abandon: Some(abandon),
        }));
        let lease = DispatchLease(Arc::clone(&state));
        let execute_state = Arc::clone(&state);
        let job = Box::new(move || {
            let _lease = lease;
            let work = {
                let mut state = execute_state.lock().unwrap_or_else(|poison| poison.into_inner());
                state.started = true;
                state.payload.take().zip(state.run.take())
            };
            if let Some((payload, run)) = work {
                run(payload);
            }
        });

        match self.port.try_dispatch_with_status(job) {
            Ok(()) => {
                let abandoned = {
                    let mut state = state.lock().unwrap_or_else(|poison| poison.into_inner());
                    state.accepted = true;
                    take_abandoned(&mut state)
                };
                if let Some((payload, abandon)) = abandoned {
                    abandon(payload);
                }
                Ok(())
            }
            Err(failure) => {
                let kind = failure.kind();
                let payload = {
                    let mut state = state.lock().unwrap_or_else(|poison| poison.into_inner());
                    state.rejected = true;
                    state.payload.take().expect("a rejected dispatch returns its original job")
                };
                drop(failure.into_job());
                Err((kind, payload))
            }
        }
    }
}

struct GuardState<T, Run, Abandon> {
    accepted: bool,
    rejected: bool,
    started: bool,
    dropped: bool,
    payload: Option<T>,
    run: Option<Run>,
    abandon: Option<Abandon>,
}

struct DispatchLease<T, Run, Abandon: FnOnce(T)>(Arc<Mutex<GuardState<T, Run, Abandon>>>);

impl<T, Run, Abandon> Drop for DispatchLease<T, Run, Abandon>
where
    Abandon: FnOnce(T),
{
    fn drop(&mut self) {
        let abandoned = {
            let mut state = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
            state.dropped = true;
            take_abandoned(&mut state)
        };
        if let Some((payload, abandon)) = abandoned {
            abandon(payload);
        }
    }
}

fn take_abandoned<T, Run, Abandon>(state: &mut GuardState<T, Run, Abandon>) -> Option<(T, Abandon)> {
    if state.accepted && state.dropped && !state.started && !state.rejected {
        state.payload.take().zip(state.abandon.take())
    } else {
        None
    }
}

#[cfg(test)]
mod guarded_dispatch_tests {
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;

    use crate::MimeError;
    use crate::detector::MimeExecutionDispatchFailure;
    use crate::detector::MimeExecutionDispatchFailureKind;
    use crate::detector::MimeExecutionPort;

    use super::MimeExecutionWorker;

    enum PortMode {
        DropBeforeReturn,
        Store(Arc<Mutex<Option<Box<dyn FnOnce() + Send + 'static>>>>),
        Reject,
        RunBeforeReturn,
    }

    impl std::fmt::Debug for PortMode {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("PortMode")
        }
    }

    impl MimeExecutionPort for PortMode {
        fn prepare(&self) -> Result<(), MimeError> {
            Ok(())
        }

        fn try_dispatch(
            &self,
            job: Box<dyn FnOnce() + Send + 'static>,
        ) -> Result<(), Box<dyn FnOnce() + Send + 'static>> {
            match self {
                Self::DropBeforeReturn => {
                    drop(job);
                    Ok(())
                }
                Self::Store(slot) => {
                    *slot.lock().unwrap() = Some(job);
                    Ok(())
                }
                Self::Reject => Err(job),
                Self::RunBeforeReturn => {
                    job();
                    Ok(())
                }
            }
        }

        fn try_dispatch_with_status(
            &self,
            job: Box<dyn FnOnce() + Send + 'static>,
        ) -> Result<(), MimeExecutionDispatchFailure> {
            self.try_dispatch(job).map_err(|job| {
                MimeExecutionDispatchFailure::new(MimeExecutionDispatchFailureKind::QueueFull, job)
            })
        }

        fn shutdown(&self) -> Result<(), MimeError> {
            Ok(())
        }
    }

    #[test]
    fn accepted_job_destroyed_before_return_publishes_once() {
        let worker = MimeExecutionWorker::new(Arc::new(PortMode::DropBeforeReturn));
        let abandoned = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&abandoned);
        assert!(worker.try_dispatch_guarded(7, |_| {}, move |value| {
            assert_eq!(7, value);
            observed.fetch_add(1, Ordering::SeqCst);
        }).is_ok());
        assert_eq!(1, abandoned.load(Ordering::SeqCst));
    }

    #[test]
    fn accepted_job_destroyed_after_return_publishes_once() {
        let slot = Arc::new(Mutex::new(None));
        let worker = MimeExecutionWorker::new(Arc::new(PortMode::Store(Arc::clone(&slot))));
        let abandoned = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&abandoned);
        worker
            .try_dispatch_guarded(11, |_| {}, move |_| {
                observed.fetch_add(1, Ordering::SeqCst);
            })
            .unwrap();
        drop(slot.lock().unwrap().take());
        assert_eq!(1, abandoned.load(Ordering::SeqCst));
    }

    #[test]
    fn synchronous_rejection_returns_payload_and_disarms_abandonment() {
        let worker = MimeExecutionWorker::new(Arc::new(PortMode::Reject));
        let abandoned = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&abandoned);
        let (kind, payload) = worker
            .try_dispatch_guarded(19, |_| {}, move |_| {
                observed.fetch_add(1, Ordering::SeqCst);
            })
            .unwrap_err();
        assert_eq!(MimeExecutionDispatchFailureKind::QueueFull, kind);
        assert_eq!(19, payload);
        assert_eq!(0, abandoned.load(Ordering::SeqCst));
    }

    #[test]
    fn synchronous_execution_is_not_reported_as_abandoned() {
        let worker = MimeExecutionWorker::new(Arc::new(PortMode::RunBeforeReturn));
        let ran = Arc::new(AtomicUsize::new(0));
        let abandoned = Arc::new(AtomicUsize::new(0));
        let ran_job = Arc::clone(&ran);
        let abandoned_job = Arc::clone(&abandoned);
        worker
            .try_dispatch_guarded(23, move |value| {
                assert_eq!(23, value);
                ran_job.fetch_add(1, Ordering::SeqCst);
            }, move |_| {
                abandoned_job.fetch_add(1, Ordering::SeqCst);
            })
            .unwrap();
        assert_eq!(1, ran.load(Ordering::SeqCst));
        assert_eq!(0, abandoned.load(Ordering::SeqCst));
    }
}
