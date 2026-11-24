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

use crate::detector::MimeExecutionDispatchFailure;
use crate::detector::MimeExecutionDispatchFailureKind;
use crate::detector::MimeExecutionPort;

mod dispatch_lease;
mod guard_state;

use dispatch_lease::DispatchLease;
use guard_state::GuardState;

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
    ///
    /// # Parameters
    /// - `job`: Sendable closure accepted by the controlled execution port.
    ///
    /// # Returns
    /// `Ok(())` when accepted, or the original closure when rejected.
    #[cfg(test)]
    pub(crate) fn try_dispatch(
        &self,
        job: Box<dyn FnOnce() + Send + 'static>,
    ) -> Result<(), Box<dyn FnOnce() + Send + 'static>> {
        self.port.try_dispatch(job)
    }

    /// Dispatches a payload with a terminal action for accepted jobs that are
    /// explicitly destroyed before execution.
    ///
    /// # Type Parameters
    /// - `T`: Sendable payload transferred to the worker or abandonment
    ///   callback.
    /// - `Run`: Sendable callback that executes the payload.
    /// - `Abandon`: Sendable callback that publishes a terminal result if
    ///   dropped.
    ///
    /// # Parameters
    /// - `payload`: State owned by the dispatched job.
    /// - `run`: Callback invoked when the job starts.
    /// - `abandon`: Callback invoked when an accepted job is discarded first.
    ///
    /// # Returns
    /// `Ok(())` when accepted, or the failure kind and original payload on
    /// rejection.
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
        self.try_dispatch_guarded_with_mode(payload, run, abandon, true)
    }

    /// Dispatches guarded work through the port's raw job method.
    ///
    /// Cleanup uses this path to preserve execution ports that distinguish raw
    /// cleanup jobs from typed command and staging submissions.
    ///
    /// # Type Parameters
    /// - `T`: Sendable payload transferred to the worker or abandonment
    ///   callback.
    /// - `Run`: Sendable callback that executes the payload.
    /// - `Abandon`: Sendable callback that publishes a terminal result if
    ///   dropped.
    ///
    /// # Parameters
    /// - `payload`: State owned by the dispatched job.
    /// - `run`: Callback invoked when the job starts.
    /// - `abandon`: Callback invoked when an accepted job is discarded first.
    ///
    /// # Returns
    /// `Ok(())` when accepted, or `Unavailable` and the original payload on
    /// rejection.
    pub(crate) fn try_dispatch_guarded_raw<T, Run, Abandon>(
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
        self.try_dispatch_guarded_with_mode(payload, run, abandon, false)
    }

    /// Runs guarded dispatch while selecting which port method owns the job.
    ///
    /// # Type Parameters
    /// - `T`: Sendable payload transferred to the worker.
    /// - `Run`: Callback that executes the payload.
    /// - `Abandon`: Callback that publishes the result if accepted work is
    ///   dropped.
    ///
    /// # Parameters
    /// - `payload`: State owned by the dispatched job.
    /// - `run`: Callback invoked when the job starts.
    /// - `abandon`: Callback invoked after accepted pre-execution destruction.
    /// - `with_status`: Whether to use the typed dispatch method.
    ///
    /// # Returns
    /// `Ok(())` when accepted, or the failure kind and original payload on
    /// rejection.
    fn try_dispatch_guarded_with_mode<T, Run, Abandon>(
        &self,
        payload: T,
        run: Run,
        abandon: Abandon,
        with_status: bool,
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
        let lease = DispatchLease {
            state: Arc::clone(&state),
        };
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

        let dispatch = if with_status {
            self.port.try_dispatch_with_status(job)
        } else {
            self.port
                .try_dispatch(job)
                .map_err(|job| MimeExecutionDispatchFailure::new(MimeExecutionDispatchFailureKind::Unavailable, job))
        };
        match dispatch {
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
                    state
                        .payload
                        .take()
                        .expect("a rejected dispatch returns its original job")
                };
                drop(failure.into_job());
                Err((kind, payload))
            }
        }
    }
}

/// Takes the abandonment callback only after acceptance and pre-execution drop.
/// Takes a terminal callback only when acceptance and pre-start drop are known.
///
/// # Type Parameters
/// - `T`: Payload retained by the dispatch state.
/// - `Run`: Execution callback retained by the dispatch state.
/// - `Abandon`: Terminal callback retained by the dispatch state.
///
/// # Parameters
/// - `state`: Mutable lifecycle state for the queued job.
///
/// # Returns
/// The payload and terminal callback when abandonment is confirmed.
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

    use super::DispatchLease;
    use super::GuardState;
    use super::MimeExecutionWorker;
    use crate::MimeError;
    use crate::detector::MimeExecutionDispatchFailure;
    use crate::detector::MimeExecutionDispatchFailureKind;
    use crate::detector::MimeExecutionPort;

    type TestJob = Box<dyn FnOnce() + Send + 'static>;
    type TestJobSlot = Arc<Mutex<Option<TestJob>>>;

    enum PortMode {
        DropBeforeReturn,
        Store(TestJobSlot),
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
            self.try_dispatch(job)
                .map_err(|job| MimeExecutionDispatchFailure::new(MimeExecutionDispatchFailureKind::QueueFull, job))
        }

        fn shutdown(&self) -> Result<(), MimeError> {
            Ok(())
        }
    }

    #[test]
    fn test_accepted_job_destroyed_before_return_publishes_one_terminal_action() {
        let worker = MimeExecutionWorker::new(Arc::new(PortMode::DropBeforeReturn));
        let abandoned = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&abandoned);
        assert!(
            worker
                .try_dispatch_guarded(
                    7,
                    |_| {},
                    move |value| {
                        assert_eq!(7, value);
                        observed.fetch_add(1, Ordering::SeqCst);
                    }
                )
                .is_ok()
        );
        assert_eq!(1, abandoned.load(Ordering::SeqCst));
    }

    #[test]
    fn test_accepted_job_destroyed_after_return_publishes_one_terminal_action() {
        let slot = Arc::new(Mutex::new(None));
        let worker = MimeExecutionWorker::new(Arc::new(PortMode::Store(Arc::clone(&slot))));
        let abandoned = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&abandoned);
        worker
            .try_dispatch_guarded(
                11,
                |_| {},
                move |_| {
                    observed.fetch_add(1, Ordering::SeqCst);
                },
            )
            .unwrap();
        drop(slot.lock().unwrap().take());
        assert_eq!(1, abandoned.load(Ordering::SeqCst));
    }

    #[test]
    fn test_synchronous_rejection_returns_payload_without_abandonment() {
        let worker = MimeExecutionWorker::new(Arc::new(PortMode::Reject));
        let abandoned = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&abandoned);
        let (kind, payload) = worker
            .try_dispatch_guarded(
                19,
                |_| {},
                move |_| {
                    observed.fetch_add(1, Ordering::SeqCst);
                },
            )
            .unwrap_err();
        assert_eq!(MimeExecutionDispatchFailureKind::QueueFull, kind);
        assert_eq!(19, payload);
        assert_eq!(0, abandoned.load(Ordering::SeqCst));
    }

    #[test]
    fn test_synchronous_execution_runs_instead_of_publishing_abandonment() {
        let worker = MimeExecutionWorker::new(Arc::new(PortMode::RunBeforeReturn));
        let ran = Arc::new(AtomicUsize::new(0));
        let abandoned = Arc::new(AtomicUsize::new(0));
        let ran_job = Arc::clone(&ran);
        let abandoned_job = Arc::clone(&abandoned);
        worker
            .try_dispatch_guarded(
                23,
                move |value| {
                    assert_eq!(23, value);
                    ran_job.fetch_add(1, Ordering::SeqCst);
                },
                move |_| {
                    abandoned_job.fetch_add(1, Ordering::SeqCst);
                },
            )
            .unwrap();
        assert_eq!(1, ran.load(Ordering::SeqCst));
        assert_eq!(0, abandoned.load(Ordering::SeqCst));
    }

    #[test]
    fn test_poisoned_dispatch_state_still_publishes_accepted_abandonment() {
        let abandoned = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&abandoned);
        let state = Arc::new(Mutex::new(GuardState {
            accepted: true,
            rejected: false,
            started: false,
            dropped: false,
            payload: Some(29),
            run: Some(|_: i32| {}),
            abandon: Some(move |value| {
                assert_eq!(29, value);
                observed.fetch_add(1, Ordering::SeqCst);
            }),
        }));
        let lease = DispatchLease {
            state: Arc::clone(&state),
        };
        let poison_state = Arc::clone(&state);
        let _ = std::panic::catch_unwind(move || {
            let _state = poison_state.lock().unwrap();
            panic!("poison dispatch state for recovery coverage");
        });

        drop(lease);

        assert_eq!(1, abandoned.load(Ordering::SeqCst));
    }
}
