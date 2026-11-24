// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Bounded MIME-owned implementation of the execution port.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use super::fixed_queue::FixedQueue;
use super::queue_state::QueueState;
use super::running_workers::RunningWorkers;
use crate::MimeError;
use crate::detector::MimeExecutionDispatchFailure;
use crate::detector::MimeExecutionDispatchFailureKind;
use crate::detector::MimeExecutionPort;

/// MIME-owned bounded worker implementation used by production adapters.
pub struct FixedMimeExecutionPort {
    /// Number of worker threads to start.
    worker_count: usize,
    /// Maximum number of queued jobs.
    queue_capacity: usize,
    /// Prepared worker pool, if one has been started.
    running: Mutex<Option<RunningWorkers>>,
    /// Shared result of shutdown, held locked until all workers have exited.
    shutdown_failed: Mutex<Option<bool>>,
    /// Whether shutdown has been requested.
    stopped: AtomicBool,
}

impl std::fmt::Debug for FixedMimeExecutionPort {
    /// Formats pool configuration and lifecycle state.
    ///
    /// # Parameters
    /// - `f`: Formatter receiving the pool state.
    ///
    /// # Returns
    /// The formatter result after writing the pool configuration and state.
    ///
    /// # Errors
    /// Returns a formatting error if the formatter cannot accept the output.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FixedMimeExecutionPort")
            .field("worker_count", &self.worker_count)
            .field("queue_capacity", &self.queue_capacity)
            .field(
                "prepared",
                &self.running.lock().unwrap_or_else(|e| e.into_inner()).is_some(),
            )
            .field("stopped", &self.stopped.load(Ordering::Acquire))
            .finish()
    }
}

impl FixedMimeExecutionPort {
    /// Creates a bounded worker service without starting threads.
    ///
    /// # Parameters
    /// - `worker_count`: Number of worker threads to create on preparation,
    ///   clamped to at least one.
    /// - `queue_capacity`: Maximum number of pending jobs, clamped to at least
    ///   one.
    ///
    /// # Returns
    /// A configured port that has not started any worker threads.
    #[must_use]
    pub fn new(worker_count: usize, queue_capacity: usize) -> Self {
        Self {
            worker_count: worker_count.max(1),
            queue_capacity: queue_capacity.max(1),
            running: Mutex::new(None),
            shutdown_failed: Mutex::new(None),
            stopped: AtomicBool::new(false),
        }
    }

    /// Runs shutdown and invokes `on_contended` if the result lock is busy.
    ///
    /// The hook lets concurrent shutdown tests observe an actual failed lock
    /// attempt before checking whether the caller returned early.
    ///
    /// # Parameters
    /// - `on_contended`: Optional callback invoked after the result lock is
    ///   observed busy.
    ///
    /// # Returns
    /// The shared shutdown result.
    ///
    /// # Errors
    /// Returns `ExecutionServicesRequired` if a worker thread panicked.
    fn shutdown_inner(&self, on_contended: Option<&dyn Fn()>) -> Result<(), MimeError> {
        let mut shutdown_failed = match self.shutdown_failed.try_lock() {
            Ok(guard) => guard,
            Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => {
                if let Some(on_contended) = on_contended {
                    on_contended();
                }
                self.shutdown_failed.lock().unwrap_or_else(|e| e.into_inner())
            }
        };
        if let Some(panicked) = *shutdown_failed {
            return if panicked {
                Err(MimeError::ExecutionServicesRequired)
            } else {
                Ok(())
            };
        }

        let mut slot = self.running.lock().unwrap_or_else(|e| e.into_inner());
        self.stopped.store(true, Ordering::Release);
        let Some(running) = slot.take() else {
            *shutdown_failed = Some(false);
            return Ok(());
        };
        drop(slot);
        {
            let mut state = running.queue.state.lock().unwrap_or_else(|e| e.into_inner());
            state.closed = true;
        }
        running.queue.ready.notify_all();
        let mut panicked = false;
        for thread in running.threads {
            panicked |= thread.join().is_err();
        }
        *shutdown_failed = Some(panicked);
        if panicked {
            Err(MimeError::ExecutionServicesRequired)
        } else {
            Ok(())
        }
    }
}

impl Drop for FixedMimeExecutionPort {
    /// Closes the queue when the final port owner disappears. A worker may
    /// release that owner itself, so its own join handle is detached while
    /// every other worker is still joined.
    fn drop(&mut self) {
        let slot = self.running.get_mut().unwrap_or_else(|e| e.into_inner());
        let Some(running) = slot.take() else {
            return;
        };
        self.stopped.store(true, Ordering::Release);
        {
            let mut state = running.queue.state.lock().unwrap_or_else(|e| e.into_inner());
            state.closed = true;
        }
        running.queue.ready.notify_all();
        let current = std::thread::current().id();
        let mut panicked = false;
        for worker in running.threads {
            if worker.thread().id() != current {
                panicked |= worker.join().is_err();
            }
        }
        let result = self.shutdown_failed.get_mut().unwrap_or_else(|e| e.into_inner());
        if result.is_none() {
            *result = Some(panicked);
        }
    }
}

impl MimeExecutionPort for FixedMimeExecutionPort {
    /// Starts the fixed worker pool once and is otherwise idempotent.
    ///
    /// # Returns
    /// `Ok(())` when the pool is ready.
    ///
    /// # Errors
    /// Returns `ExecutionServicesRequired` if shutdown has begun or a worker
    /// thread cannot be started.
    fn prepare(&self) -> Result<(), MimeError> {
        let mut running = self.running.lock().unwrap_or_else(|e| e.into_inner());
        if self.stopped.load(Ordering::Acquire) {
            return Err(MimeError::ExecutionServicesRequired);
        }
        if running.is_some() {
            return Ok(());
        }
        let queue = Arc::new(FixedQueue {
            state: Mutex::new(QueueState {
                jobs: std::collections::VecDeque::with_capacity(self.queue_capacity),
                closed: false,
            }),
            ready: std::sync::Condvar::new(),
            capacity: self.queue_capacity,
        });
        let mut threads = Vec::with_capacity(self.worker_count);
        for index in 0..self.worker_count {
            let worker_queue = Arc::clone(&queue);
            match std::thread::Builder::new()
                .name(format!("mime-worker-{index}"))
                .spawn(move || worker_loop(worker_queue))
            {
                Ok(thread) => threads.push(thread),
                Err(_) => {
                    {
                        let mut state = queue.state.lock().unwrap_or_else(|e| e.into_inner());
                        state.closed = true;
                    }
                    queue.ready.notify_all();
                    for thread in threads {
                        let _ = thread.join();
                    }
                    return Err(MimeError::ExecutionServicesRequired);
                }
            }
        }
        *running = Some(RunningWorkers { queue, threads });
        Ok(())
    }

    /// Adds a job if the prepared queue has available capacity.
    ///
    /// # Parameters
    /// - `job`: Job to enqueue.
    ///
    /// # Returns
    /// `Ok(())` when queued, or the unmodified job when rejected.
    fn try_dispatch(&self, job: Box<dyn FnOnce() + Send + 'static>) -> Result<(), Box<dyn FnOnce() + Send + 'static>> {
        self.try_dispatch_with_status(job)
            .map_err(MimeExecutionDispatchFailure::into_job)
    }

    /// Adds a job and reports whether rejection came from queue saturation or
    /// service unavailability.
    ///
    /// # Parameters
    /// - `job`: Owned job to enqueue without blocking.
    ///
    /// # Returns
    /// `Ok(())` when queued, or the unmodified job and its rejection reason.
    fn try_dispatch_with_status(
        &self,
        job: Box<dyn FnOnce() + Send + 'static>,
    ) -> Result<(), MimeExecutionDispatchFailure> {
        let running = self.running.lock().unwrap_or_else(|e| e.into_inner());
        if self.stopped.load(Ordering::Acquire) {
            return Err(MimeExecutionDispatchFailure::new(
                MimeExecutionDispatchFailureKind::Unavailable,
                job,
            ));
        }
        let Some(running) = running.as_ref() else {
            return Err(MimeExecutionDispatchFailure::new(
                MimeExecutionDispatchFailureKind::Unavailable,
                job,
            ));
        };
        let mut state = running.queue.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.closed {
            return Err(MimeExecutionDispatchFailure::new(
                MimeExecutionDispatchFailureKind::Unavailable,
                job,
            ));
        }
        if state.jobs.len() >= running.queue.capacity {
            return Err(MimeExecutionDispatchFailure::new(
                MimeExecutionDispatchFailureKind::QueueFull,
                job,
            ));
        }
        state.jobs.push_back(job);
        drop(state);
        running.queue.ready.notify_one();
        Ok(())
    }

    /// Closes the queue and waits for all worker threads to exit.
    ///
    /// Concurrent calls wait for the same shutdown operation and receive its
    /// final result. Call this from a synchronous context outside this pool's
    /// worker threads; a worker cannot synchronously wait for its own pool.
    ///
    /// # Returns
    /// `Ok(())` when all workers have exited.
    ///
    /// # Errors
    /// Returns `ExecutionServicesRequired` if a worker thread panicked.
    fn shutdown(&self) -> Result<(), MimeError> {
        self.shutdown_inner(None)
    }
}

/// Runs jobs until the queue is closed and drained.
///
/// # Parameters
/// - `queue`: Queue consumed by this worker thread.
fn worker_loop(queue: Arc<FixedQueue>) {
    loop {
        let job = {
            let mut state = queue.state.lock().unwrap_or_else(|e| e.into_inner());
            while state.jobs.is_empty() && !state.closed {
                state = queue.ready.wait(state).unwrap_or_else(|e| e.into_inner());
            }
            match state.jobs.pop_front() {
                Some(job) => job,
                None if state.closed => return,
                None => continue,
            }
        };
        // Isolate panicking callbacks so they do not reduce the fixed pool.
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job));
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::Ordering;
    use std::sync::mpsc;

    use super::FixedMimeExecutionPort;
    use super::FixedQueue;
    use super::QueueState;
    use super::RunningWorkers;
    use crate::detector::MimeExecutionPort;

    #[test]
    fn test_unprepared_dispatch_returns_the_original_job_for_recovery() {
        let port = FixedMimeExecutionPort::new(1, 1);
        let ran = Arc::new(AtomicBool::new(false));
        let job_ran = Arc::clone(&ran);
        let returned = port
            .try_dispatch(Box::new(move || job_ran.store(true, Ordering::SeqCst)))
            .expect_err("unprepared port must reject dispatch");

        assert!(!ran.load(Ordering::SeqCst));
        returned();
        assert!(ran.load(Ordering::SeqCst));
        port.shutdown().unwrap();
    }

    #[test]
    fn test_shutdown_drains_queued_jobs_and_contains_worker_panics() {
        let port = Arc::new(FixedMimeExecutionPort::new(1, 2));
        port.prepare().unwrap();
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let drained = Arc::new(AtomicBool::new(false));
        let first_release = release_rx;
        assert!(
            port.try_dispatch(Box::new(move || {
                started_tx.send(()).unwrap();
                first_release.recv().unwrap();
                panic!("isolated worker callback panic");
            }))
            .is_ok()
        );
        started_rx.recv().unwrap();
        let queued_ran = Arc::clone(&drained);
        assert!(
            port.try_dispatch(Box::new(move || queued_ran.store(true, Ordering::SeqCst)))
                .is_ok()
        );

        let shutdown_port = Arc::clone(&port);
        let shutdown = std::thread::spawn(move || shutdown_port.shutdown());
        release_tx.send(()).unwrap();
        shutdown.join().unwrap().unwrap();
        assert!(drained.load(Ordering::SeqCst), "shutdown must drain accepted jobs");
        let rejected_ran = Arc::new(AtomicBool::new(false));
        let rejected_job_ran = Arc::clone(&rejected_ran);
        let rejected = port
            .try_dispatch(Box::new(move || rejected_job_ran.store(true, Ordering::SeqCst)))
            .expect_err("shutdown is terminal");
        assert!(!rejected_ran.load(Ordering::SeqCst));
        rejected();
        assert!(
            rejected_ran.load(Ordering::SeqCst),
            "shutdown returns ownership of rejected work"
        );
    }

    #[test]
    fn test_last_port_owner_can_be_released_by_its_worker() {
        let port = Arc::new(FixedMimeExecutionPort::new(1, 1));
        port.prepare().unwrap();
        let weak = Arc::downgrade(&port);
        let worker_owner = Arc::clone(&port);
        let (dropped_tx, dropped_rx) = mpsc::channel();
        assert!(
            port.try_dispatch(Box::new(move || {
                drop(worker_owner);
                dropped_tx.send(()).unwrap();
            }))
            .is_ok()
        );
        drop(port);

        dropped_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("worker-side final drop must not join itself");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while weak.upgrade().is_some() && std::time::Instant::now() < deadline {
            std::thread::yield_now();
        }
        assert!(weak.upgrade().is_none(), "last worker owner should be released");
    }

    #[test]
    fn test_concurrent_shutdown_waits_for_actual_worker_exit() {
        let port = Arc::new(FixedMimeExecutionPort::new(1, 1));
        port.prepare().unwrap();
        let queue = Arc::clone(
            &port
                .running
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .as_ref()
                .unwrap()
                .queue,
        );
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        assert!(
            port.try_dispatch(Box::new(move || {
                entered_tx.send(()).unwrap();
                release_rx.recv().unwrap();
            }))
            .is_ok()
        );
        entered_rx.recv().unwrap();

        let (first_done_tx, first_done_rx) = mpsc::channel();
        let first_port = Arc::clone(&port);
        let first = std::thread::spawn(move || {
            let result = first_port.shutdown();
            first_done_tx.send(result).unwrap();
        });

        // Wait until shutdown has taken the worker handles and closed the
        // queue. At that point its join is necessarily blocked on the job.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        let mut reached_worker_join = false;
        loop {
            let running_is_empty = port.running.lock().unwrap_or_else(|e| e.into_inner()).is_none();
            let queue_is_closed = queue.state.lock().unwrap_or_else(|e| e.into_inner()).closed;
            if running_is_empty && queue_is_closed {
                reached_worker_join = true;
                break;
            }
            if std::time::Instant::now() >= deadline {
                break;
            }
            std::thread::yield_now();
        }

        let (second_contended_tx, second_contended_rx) = mpsc::channel();
        let (second_done_tx, second_done_rx) = mpsc::channel();
        let second_port = Arc::clone(&port);
        let second = std::thread::spawn(move || {
            let on_contended = || second_contended_tx.send(()).unwrap();
            let result = second_port.shutdown_inner(Some(&on_contended));
            second_done_tx.send(result).unwrap();
        });
        let observed_lock_contention = second_contended_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .is_ok();
        let second_returned_early = second_done_rx
            .recv_timeout(std::time::Duration::from_millis(100))
            .is_ok();

        let dispatched = port.try_dispatch(Box::new(|| {}));
        let dispatch_was_rejected = dispatched.is_err();
        if let Err(job) = dispatched {
            job();
        }

        // Always release and join before asserting so a failure cannot leave
        // a blocked worker or shutdown thread behind.
        release_tx.send(()).unwrap();
        let first_result = first_done_rx.recv_timeout(std::time::Duration::from_secs(2));
        let second_result = if second_returned_early {
            second_done_rx.try_recv().ok()
        } else {
            second_done_rx.recv_timeout(std::time::Duration::from_secs(2)).ok()
        };
        first.join().unwrap();
        second.join().unwrap();

        assert!(
            reached_worker_join,
            "shutdown did not reach worker join before the timeout"
        );
        assert!(
            observed_lock_contention,
            "second shutdown did not contend on the first shutdown result lock"
        );
        assert!(
            !second_returned_early,
            "concurrent shutdown returned before workers exited"
        );
        assert!(dispatch_was_rejected, "dispatch remains available during shutdown");
        first_result.unwrap().unwrap();
        second_result.unwrap().unwrap();
    }

    #[test]
    fn test_zero_worker_and_queue_configuration_is_clamped_to_one() {
        let port = FixedMimeExecutionPort::new(0, 0);
        let debug = format!("{port:?}");
        assert!(debug.contains("worker_count: 1"));
        assert!(debug.contains("queue_capacity: 1"));
        port.prepare().unwrap();
        port.shutdown().unwrap();
    }

    #[test]
    fn test_repeated_prepare_keeps_one_pool_accepting_jobs() {
        let port = FixedMimeExecutionPort::new(1, 1);
        port.prepare().unwrap();
        port.prepare().unwrap();
        let (done_tx, done_rx) = mpsc::channel();
        assert!(port.try_dispatch(Box::new(move || done_tx.send(41).unwrap())).is_ok());
        assert_eq!(41, done_rx.recv().unwrap());
        port.shutdown().unwrap();
    }

    #[test]
    fn test_poisoned_pool_lock_recovers_for_debug_prepare_dispatch_and_shutdown() {
        let port = FixedMimeExecutionPort::new(1, 1);
        let poison = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = port.running.lock().unwrap();
            panic!("poison worker pool state before preparation");
        }));
        assert!(poison.is_err());
        assert!(format!("{port:?}").contains("prepared: false"));
        port.prepare().unwrap();

        let (done_tx, done_rx) = mpsc::channel();
        assert!(port.try_dispatch(Box::new(move || done_tx.send(7).unwrap())).is_ok());
        assert_eq!(7, done_rx.recv().unwrap());
        port.shutdown().unwrap();
        assert!(format!("{port:?}").contains("stopped: true"));

        let result_lock_port = FixedMimeExecutionPort::new(1, 1);
        let result_lock_poison = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = result_lock_port.shutdown_failed.lock().unwrap();
            panic!("poison shutdown result before closing an unprepared pool");
        }));
        assert!(result_lock_poison.is_err());
        result_lock_port.shutdown().unwrap();
        result_lock_port.shutdown().unwrap();

        let queue_lock_port = FixedMimeExecutionPort::new(1, 1);
        let queue = Arc::new(FixedQueue {
            state: std::sync::Mutex::new(QueueState {
                jobs: std::collections::VecDeque::new(),
                closed: false,
            }),
            ready: std::sync::Condvar::new(),
            capacity: 1,
        });
        let queue_to_poison = Arc::clone(&queue);
        let queue_lock_poison = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = queue_to_poison.state.lock().unwrap();
            panic!("poison queue state before shutdown");
        }));
        assert!(queue_lock_poison.is_err());
        *queue_lock_port.running.lock().unwrap() = Some(RunningWorkers {
            queue,
            threads: Vec::new(),
        });
        queue_lock_port.shutdown().unwrap();
    }

    #[test]
    fn test_dispatch_recovers_poisoned_queue_and_runs_the_returned_work() {
        let port = FixedMimeExecutionPort::new(1, 1);
        port.prepare().unwrap();
        let queue = port
            .running
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .queue
            .clone();
        let queue_to_poison = Arc::clone(&queue);
        assert!(
            std::thread::spawn(move || {
                let _guard = queue_to_poison.state.lock().unwrap();
                panic!("poison queue before dispatch");
            })
            .join()
            .is_err()
        );

        let (done_tx, done_rx) = mpsc::channel();
        assert!(port
            .try_dispatch_with_status(Box::new(move || done_tx.send(()).unwrap()))
            .is_ok());
        done_rx.recv().unwrap();
        port.shutdown().unwrap();
    }

    #[test]
    fn test_closed_queue_returns_the_unaccepted_job_while_port_is_open() {
        let port = FixedMimeExecutionPort::new(1, 1);
        port.prepare().unwrap();
        let queue = port
            .running
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .queue
            .clone();
        queue.state.lock().unwrap().closed = true;
        queue.ready.notify_all();

        let ran = Arc::new(AtomicBool::new(false));
        let job_ran = Arc::clone(&ran);
        let rejected = port
            .try_dispatch_with_status(Box::new(move || job_ran.store(true, Ordering::SeqCst)))
            .expect_err("a closed queue cannot accept work");
        assert_eq!(
            crate::detector::MimeExecutionDispatchFailureKind::Unavailable,
            rejected.kind()
        );
        assert!(!ran.load(Ordering::SeqCst));
        rejected.into_job()();
        assert!(ran.load(Ordering::SeqCst));
        port.shutdown().unwrap();
    }

    #[test]
    fn test_drop_recovers_poisoned_pool_queue_and_shutdown_locks() {
        let port = Arc::new(FixedMimeExecutionPort::new(1, 1));
        let queue = Arc::new(FixedQueue {
            state: std::sync::Mutex::new(QueueState {
                jobs: std::collections::VecDeque::new(),
                closed: false,
            }),
            ready: std::sync::Condvar::new(),
            capacity: 1,
        });
        *port.running.lock().unwrap() = Some(RunningWorkers {
            queue: Arc::clone(&queue),
            threads: Vec::new(),
        });

        let pool_to_poison = Arc::clone(&port);
        assert!(
            std::thread::spawn(move || {
                let _guard = pool_to_poison.running.lock().unwrap();
                panic!("poison pool state before drop");
            })
            .join()
            .is_err()
        );
        let result_to_poison = Arc::clone(&port);
        assert!(
            std::thread::spawn(move || {
                let _guard = result_to_poison.shutdown_failed.lock().unwrap();
                panic!("poison shutdown result before drop");
            })
            .join()
            .is_err()
        );
        let queue_to_poison = Arc::clone(&queue);
        assert!(
            std::thread::spawn(move || {
                let _guard = queue_to_poison.state.lock().unwrap();
                panic!("poison queue state before drop");
            })
            .join()
            .is_err()
        );

        drop(port);
        let state = match queue.state.lock() {
            Ok(_) => panic!("drop should preserve the queue's poison marker"),
            Err(poisoned) => poisoned.into_inner(),
        };
        assert!(state.closed, "drop closes a queue despite poisoned locks");
    }

    #[test]
    fn test_shutdown_prevents_reprepare_and_returns_later_work() {
        let port = FixedMimeExecutionPort::new(1, 1);
        port.prepare().unwrap();
        port.shutdown().unwrap();
        port.shutdown().unwrap();

        assert!(port.prepare().is_err(), "shutdown permanently closes preparation");
        let ran = Arc::new(AtomicBool::new(false));
        let job_ran = Arc::clone(&ran);
        let returned = port
            .try_dispatch(Box::new(move || job_ran.store(true, Ordering::SeqCst)))
            .expect_err("a terminal port must return ownership of new work");
        assert!(!ran.load(Ordering::SeqCst));
        returned();
        assert!(ran.load(Ordering::SeqCst));
    }

    #[test]
    fn test_repeated_shutdown_replays_cached_worker_failure() {
        let port = FixedMimeExecutionPort::new(1, 1);
        let (panicked_tx, panicked_rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            panicked_tx.send(()).unwrap();
            panic!("inject a worker join failure, not a caught job panic");
        });
        panicked_rx.recv().unwrap();
        let queue = Arc::new(FixedQueue {
            state: std::sync::Mutex::new(QueueState {
                jobs: std::collections::VecDeque::new(),
                closed: false,
            }),
            ready: std::sync::Condvar::new(),
            capacity: 1,
        });
        *port.running.lock().unwrap() = Some(RunningWorkers {
            queue,
            threads: vec![worker],
        });

        assert!(matches!(
            port.shutdown(),
            Err(crate::MimeError::ExecutionServicesRequired)
        ));
        assert!(matches!(
            port.shutdown(),
            Err(crate::MimeError::ExecutionServicesRequired)
        ));
    }

    #[test]
    fn test_two_workers_run_accepted_jobs_concurrently_before_shutdown() {
        let port = FixedMimeExecutionPort::new(2, 2);
        port.prepare().unwrap();
        let barrier = Arc::new(std::sync::Barrier::new(3));
        let (done_tx, done_rx) = mpsc::channel();

        for _ in 0..2 {
            let worker_barrier = Arc::clone(&barrier);
            let done_tx = done_tx.clone();
            assert!(
                port.try_dispatch(Box::new(move || {
                    worker_barrier.wait();
                    worker_barrier.wait();
                    done_tx.send(()).unwrap();
                }))
                .is_ok()
            );
        }
        drop(done_tx);

        barrier.wait();
        barrier.wait();
        done_rx.recv().unwrap();
        done_rx.recv().unwrap();
        assert!(done_rx.try_recv().is_err());
        port.shutdown().unwrap();
    }

    #[test]
    fn test_full_queue_returns_unaccepted_job_while_accepted_work_drains() {
        let port = FixedMimeExecutionPort::new(1, 1);
        port.prepare().unwrap();
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        assert!(
            port.try_dispatch(Box::new(move || {
                started_tx.send(()).unwrap();
                release_rx.recv().unwrap();
            }))
            .is_ok()
        );
        started_rx.recv().unwrap();

        let accepted_ran = Arc::new(AtomicBool::new(false));
        let accepted_job_ran = Arc::clone(&accepted_ran);
        assert!(
            port.try_dispatch(Box::new(move || accepted_job_ran.store(true, Ordering::SeqCst)))
                .is_ok()
        );

        let rejected_ran = Arc::new(AtomicBool::new(false));
        let rejected_job_ran = Arc::clone(&rejected_ran);
        let rejected = port
            .try_dispatch_with_status(Box::new(move || rejected_job_ran.store(true, Ordering::SeqCst)))
            .expect_err("capacity is occupied by the accepted queued job");
        assert_eq!(
            crate::detector::MimeExecutionDispatchFailureKind::QueueFull,
            rejected.kind(),
        );
        assert!(!rejected_ran.load(Ordering::SeqCst));
        rejected.into_job()();
        assert!(rejected_ran.load(Ordering::SeqCst), "rejection preserves job ownership");
        assert!(!accepted_ran.load(Ordering::SeqCst));

        release_tx.send(()).unwrap();
        port.shutdown().unwrap();
        assert!(
            accepted_ran.load(Ordering::SeqCst),
            "shutdown drains previously accepted work"
        );
    }
}
