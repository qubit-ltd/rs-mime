// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::future::Future;
use std::sync::Arc;
use std::task::Context;
use std::task::Poll;
use std::task::Wake;
use std::task::Waker;

use qubit_mime::AsyncMimeDetectorBackend;
use qubit_mime::BorrowedMimeInput;
use qubit_mime::FileCommandMimeDetector;
use qubit_mime::InputExtent;
use qubit_mime::MimeOperationOptions;

struct ThreadWake(std::thread::Thread);

impl Wake for ThreadWake {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }

    fn wake_by_ref(self: &Arc<Self>) {
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
fn test_file_operation_complete_only_rejects_prefix() {
    let detector = FileCommandMimeDetector::new();
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let group = manager.group(qubit_operation::GroupOptions { capacity: 1 }).unwrap();
    let mut handle = group
        .as_async()
        .try_submit(Default::default(), move |scope| async move {
            detector
                .guess_content_in(
                    &scope,
                    BorrowedMimeInput::Bytes {
                        content: b"%PDF",
                        extent: InputExtent::Prefix,
                        filename: None,
                    },
                    &MimeOperationOptions::default(),
                )
                .await
                .map(|_| ())
        })
        .unwrap();
    let result = block_on(handle.wait_outcome()).unwrap();
    assert!(matches!(
        result,
        qubit_operation::OperationOutcome::Failed(qubit_mime::MimeError::CompleteContentRequired)
    ));
}

#[test]
fn test_file_operation_declares_complete_content_and_no_random_access() {
    let detector = FileCommandMimeDetector::new();
    assert_eq!(detector.content_requirement(), qubit_mime::ContentRequirement::Complete);
    assert_eq!(
        detector.random_access_capability(),
        qubit_mime::RandomAccessCapability::Unsupported
    );
}

#[test]
fn test_file_operation_cancelled_while_queued_releases_all_resources() {
    let mut manager_options = qubit_operation::ManagerOptions::default();
    manager_options.limits.active_operations = 2;
    manager_options.limits.queued_operations = 1;
    manager_options.async_executor.worker_count = 1;
    let manager = qubit_operation::OperationManager::new(manager_options).unwrap();
    let group = manager.group(qubit_operation::GroupOptions { capacity: 2 }).unwrap();

    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let mut blocker = group
        .as_async()
        .try_submit(Default::default(), move |_| async move {
            entered_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            Ok::<_, qubit_mime::MimeError>(())
        })
        .unwrap();
    entered_rx.recv().unwrap();

    let detector = FileCommandMimeDetector::new();
    let mut queued = group
        .as_async()
        .try_submit(Default::default(), move |scope| async move {
            detector
                .guess_content_in(
                    &scope,
                    BorrowedMimeInput::Bytes {
                        content: b"complete input",
                        extent: InputExtent::Complete,
                        filename: None,
                    },
                    &MimeOperationOptions::default(),
                )
                .await
                .map(|_| ())
        })
        .unwrap();
    assert_eq!(
        queued.observer().snapshot().execution,
        qubit_operation::ExecutionPhase::Queued
    );

    let _ = queued.request_stop(qubit_operation::StopReason::UserCancelled);
    release_tx.send(()).unwrap();

    assert!(matches!(
        block_on(blocker.wait_outcome()).unwrap(),
        qubit_operation::OperationOutcome::Succeeded(())
    ));
    assert!(matches!(
        block_on(queued.wait_outcome()).unwrap(),
        qubit_operation::OperationOutcome::Stopped(_)
    ));
    assert!(block_on(queued.wait_settlement()).all_released());
}
