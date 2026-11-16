// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Nonblocking dispatch into the MIME execution service's bounded workers.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Condvar;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::thread::JoinHandle;

use crate::detector::MimeExecutionPort;

/// Cloneable handle to the service-owned fixed-capacity worker dispatcher.
#[derive(Clone)]
pub(crate) struct MimeExecutionWorker {
    port: Arc<dyn MimeExecutionPort>,
}

type Job = Box<dyn FnOnce() + Send + 'static>;

struct QueueState {
    jobs: VecDeque<Job>,
    closed: bool,
}

struct FixedQueue {
    state: Mutex<QueueState>,
    ready: Condvar,
    capacity: usize,
}

struct RunningWorkers {
    queue: Arc<FixedQueue>,
    threads: Vec<JoinHandle<()>>,
}

/// MIME-owned bounded worker implementation used by production adapters.
pub struct FixedMimeExecutionPort {
    worker_count: usize,
    queue_capacity: usize,
    running: Mutex<Option<RunningWorkers>>,
    stopped: AtomicBool,
}

impl std::fmt::Debug for FixedMimeExecutionPort {
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
    pub fn new(worker_count: usize, queue_capacity: usize) -> Self {
        Self {
            worker_count: worker_count.max(1),
            queue_capacity: queue_capacity.max(1),
            running: Mutex::new(None),
            stopped: AtomicBool::new(false),
        }
    }
}

impl crate::detector::MimeExecutionPort for FixedMimeExecutionPort {
    fn prepare(&self) -> Result<(), crate::MimeError> {
        let mut running = self.running.lock().unwrap_or_else(|e| e.into_inner());
        if self.stopped.load(Ordering::Acquire) {
            return Err(crate::MimeError::ExecutionServicesRequired);
        }
        if running.is_some() {
            return Ok(());
        }
        let queue = Arc::new(FixedQueue {
            state: Mutex::new(QueueState {
                jobs: VecDeque::with_capacity(self.queue_capacity),
                closed: false,
            }),
            ready: Condvar::new(),
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
                    return Err(crate::MimeError::ExecutionServicesRequired);
                }
            }
        }
        *running = Some(RunningWorkers { queue, threads });
        Ok(())
    }

    fn try_dispatch(&self, job: Job) -> Result<(), Job> {
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

    fn shutdown(&self) -> Result<(), crate::MimeError> {
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
            Err(crate::MimeError::ExecutionServicesRequired)
        } else {
            Ok(())
        }
    }
}

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
        // A panicking domain callback must not reduce the fixed worker count.
        // Each MIME job also owns its own completion guard so its observer is
        // completed (or its resource returned) before the panic is contained.
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job));
    }
}

impl MimeExecutionWorker {
    /// Uses the dispatch service already owned by the application.
    pub(crate) fn new(port: Arc<dyn MimeExecutionPort>) -> Self {
        Self { port }
    }

    /// Offers a job without blocking and returns its exact ownership on reject.
    pub(crate) fn try_dispatch(
        &self,
        job: Box<dyn FnOnce() + Send + 'static>,
    ) -> Result<(), Box<dyn FnOnce() + Send + 'static>> {
        self.port.try_dispatch(job)
    }
}
