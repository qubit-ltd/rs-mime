// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

//! The execution port preserves dispatch ownership across lifecycle changes.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use qubit_mime::MimeError;
use qubit_mime::MimeExecutionPort;

#[derive(Debug)]
struct RejectingPort;

impl MimeExecutionPort for RejectingPort {
    fn prepare(&self) -> Result<(), MimeError> {
        Ok(())
    }

    fn shutdown(&self) -> Result<(), MimeError> {
        Ok(())
    }
}

#[derive(Debug)]
struct AcceptingPort {
    prepared: AtomicBool,
    stopped: AtomicBool,
}

impl MimeExecutionPort for AcceptingPort {
    fn prepare(&self) -> Result<(), MimeError> {
        self.prepared.store(true, Ordering::SeqCst);
        Ok(())
    }

    fn try_dispatch(&self, job: Box<dyn FnOnce() + Send + 'static>) -> Result<(), Box<dyn FnOnce() + Send + 'static>> {
        if !self.prepared.load(Ordering::SeqCst) || self.stopped.load(Ordering::SeqCst) {
            return Err(job);
        }
        job();
        Ok(())
    }

    fn shutdown(&self) -> Result<(), MimeError> {
        self.stopped.store(true, Ordering::SeqCst);
        Ok(())
    }
}

#[test]
fn test_execution_port_accepts_jobs_only_during_prepared_lifecycle() {
    let port = AcceptingPort {
        prepared: AtomicBool::new(false),
        stopped: AtomicBool::new(false),
    };
    let calls = Arc::new(AtomicBool::new(false));
    let first = Arc::clone(&calls);
    let rejected = port
        .try_dispatch(Box::new(move || first.store(true, Ordering::SeqCst)))
        .unwrap_err();
    assert!(!calls.load(Ordering::SeqCst));
    port.prepare().unwrap();
    let accepted = Arc::clone(&calls);
    assert!(
        port.try_dispatch(Box::new(move || accepted.store(true, Ordering::SeqCst)))
            .is_ok()
    );
    assert!(calls.load(Ordering::SeqCst));
    port.shutdown().unwrap();
    let after_stop = Arc::clone(&calls);
    assert!(
        port.try_dispatch(Box::new(move || after_stop.store(false, Ordering::SeqCst)))
            .is_err()
    );
    rejected();
    assert!(calls.load(Ordering::SeqCst), "rejected job remains caller-owned");
}

#[test]
fn test_default_dispatch_rejection_returns_unconsumed_job() {
    RejectingPort.prepare().unwrap();
    let called = Arc::new(AtomicBool::new(false));
    let job_called = Arc::clone(&called);
    let job = Box::new(move || job_called.store(true, Ordering::SeqCst));
    let returned = RejectingPort
        .try_dispatch(job)
        .expect_err("default port does not accept work");
    assert!(!called.load(Ordering::SeqCst));
    returned();
    assert!(called.load(Ordering::SeqCst));
    RejectingPort.shutdown().unwrap();
}
