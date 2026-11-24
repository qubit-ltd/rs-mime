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
use std::time::Duration;
use std::time::Instant;

use qubit_mime::AsyncMediaStreamClassifierProvider;
use qubit_mime::BorrowedMimeInput;
use qubit_mime::FfprobeCommandMediaStreamClassifierProvider;
use qubit_mime::FixedMimeExecutionPort;
use qubit_mime::InputExtent;
use qubit_mime::MediaStreamType;
use qubit_mime::MimeConfig;
use qubit_mime::MimeExecutionServices;
use qubit_mime::MimeOperationOptions;
use qubit_operation::OperationOutcome;

const REAL_OGG: &[u8] = include_bytes!("../fixtures/real_files/test.ogg");
const TEST_TIMEOUT: Duration = Duration::from_secs(90);

struct ThreadWake(std::thread::Thread);

impl Wake for ThreadWake {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}

fn block_on_timeout<F: Future>(future: F, timeout: Duration) -> Option<F::Output> {
    let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    let deadline = Instant::now() + timeout;

    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(value) => return Some(value),
            Poll::Pending => {
                let now = Instant::now();
                if now >= deadline {
                    return None;
                }
                std::thread::park_timeout(deadline - now);
            }
        }
    }
}

#[test]
#[ignore = "requires an explicitly prepared FFprobe binary and libraries"]
fn test_classifies_ogg_with_managed_ffprobe_command() {
    assert!(!REAL_OGG.is_empty(), "the real Ogg fixture must be present");

    let manager = qubit_operation::OperationManager::new(Default::default()).expect("create operation manager");
    let group = manager
        .group(qubit_operation::GroupOptions { capacity: 2 })
        .expect("create operation group");
    let services = Arc::new(
        MimeExecutionServices::new(
            manager.clone(),
            group.clone(),
            Arc::new(FixedMimeExecutionPort::new(1, 2)),
            REAL_OGG.len() as u64,
        )
        .expect("create managed MIME execution services"),
    );
    services.prepare().expect("prepare managed MIME execution services");

    let backend = Arc::new(
        FfprobeCommandMediaStreamClassifierProvider
            .create_async_backend(&MimeConfig::default(), &services)
            .expect("create managed FFprobe classifier"),
    );
    let operation_backend = Arc::clone(&backend);
    let mut operation = group
        .as_async()
        .try_submit(Default::default(), move |scope| async move {
            operation_backend
                .classify_in(
                    &scope,
                    BorrowedMimeInput::Bytes {
                        content: REAL_OGG,
                        extent: InputExtent::Complete,
                        filename: Some("test.ogg"),
                    },
                    &MimeOperationOptions::default(),
                )
                .await
        })
        .expect("submit FFprobe classification");

    let outcome = block_on_timeout(operation.wait_outcome(), TEST_TIMEOUT)
        .expect("FFprobe operation exceeded its bounded test wait");
    assert!(
        matches!(outcome, Ok(OperationOutcome::Succeeded(MediaStreamType::AudioOnly))),
        "real FFprobe must classify the Ogg fixture as AudioOnly; got {outcome:?}; ensure ffprobe is on PATH and its shared libraries are available"
    );

    let settlement = block_on_timeout(operation.wait_settlement(), TEST_TIMEOUT)
        .expect("FFprobe settlement exceeded its bounded test wait");
    assert!(
        settlement.all_released(),
        "FFprobe operation must settle: {settlement:?}"
    );

    let shutdown = services.shutdown();
    let settled = block_on_timeout(shutdown.wait_services_settled(), TEST_TIMEOUT)
        .expect("managed service shutdown exceeded its bounded test wait")
        .expect("managed services should settle");
    assert!(settled.operations_settled);
    let stopped = shutdown.stop_services().expect("stop managed services");
    assert!(stopped.operations_settled && stopped.services_stopped);
    let report = block_on_timeout(shutdown.wait_group_shutdown(), TEST_TIMEOUT)
        .expect("operation group shutdown exceeded its bounded test wait")
        .expect("operation group should shut down");
    assert_eq!(report.snapshot.active_operations, 0);
}
