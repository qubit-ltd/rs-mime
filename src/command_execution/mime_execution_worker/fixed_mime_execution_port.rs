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
use crate::detector::MimeExecutionPort;

/// MIME-owned bounded worker implementation used by production adapters.
pub struct FixedMimeExecutionPort {
    /// Number of worker threads to start.
    worker_count: usize,
    /// Maximum number of queued jobs.
    queue_capacity: usize,
    /// Prepared worker pool, if one has been started.
    running: Mutex<Option<RunningWorkers>>,
    /// Whether shutdown has been requested.
    stopped: AtomicBool,
}

impl std::fmt::Debug for FixedMimeExecutionPort {
    /// Formats pool configuration and lifecycle state.
    ///
    /// # Parameters
    /// - `f`: Formatter receiving the pool state.
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
    /// - `worker_count`: Number of worker threads to create on preparation.
    /// - `queue_capacity`: Maximum number of pending jobs.
    pub fn new(worker_count: usize, queue_capacity: usize) -> Self {
        Self {
            worker_count: worker_count.max(1),
            queue_capacity: queue_capacity.max(1),
            running: Mutex::new(None),
            stopped: AtomicBool::new(false),
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
        let running = self.running.lock().unwrap_or_else(|e| e.into_inner());
        if self.stopped.load(Ordering::Acquire) {
            return Err(job);
        }
        let Some(running) = running.as_ref() else {
            return Err(job);
        };
        let mut state = running.queue.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.closed || state.jobs.len() >= running.queue.capacity {
            return Err(job);
        }
        state.jobs.push_back(job);
        drop(state);
        running.queue.ready.notify_one();
        Ok(())
    }

    /// Closes the queue and waits for all worker threads to exit.
    ///
    /// # Returns
    /// `Ok(())` when all workers have exited.
    ///
    /// # Errors
    /// Returns `ExecutionServicesRequired` if a worker thread panicked.
    fn shutdown(&self) -> Result<(), MimeError> {
        let mut slot = self.running.lock().unwrap_or_else(|e| e.into_inner());
        self.stopped.store(true, Ordering::Release);
        let Some(running) = slot.take() else {
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
        if panicked {
            Err(MimeError::ExecutionServicesRequired)
        } else {
            Ok(())
        }
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
