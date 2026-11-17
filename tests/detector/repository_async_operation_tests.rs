// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::Context;
use std::task::Poll;
use std::task::Wake;
use std::task::Waker;

use qubit_mime::AsyncMimeDetectorBackend;
use qubit_mime::BorrowedMimeInput;
use qubit_mime::InputExtent;
use qubit_mime::InspectedExtent;
use qubit_mime::MimeOperationOptions;
use qubit_mime::MimeRepository;
use qubit_mime::RepositoryMimeDetector;

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
    let mut cx = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    loop {
        match Pin::as_mut(&mut future).poll(&mut cx) {
            Poll::Ready(value) => return value,
            Poll::Pending => std::thread::park(),
        }
    }
}

fn run_backend(
    backend: Arc<dyn AsyncMimeDetectorBackend>,
    bytes: Arc<[u8]>,
    extent: InputExtent,
    filename: Option<String>,
    policy: qubit_mime::MimeDetectionPolicy,
) -> qubit_mime::MimeCandidates {
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let group = manager.group(qubit_operation::GroupOptions { capacity: 1 }).unwrap();
    let mut handle = group
        .as_async()
        .try_submit(Default::default(), move |scope| async move {
            backend
                .guess_content_in(
                    &scope,
                    BorrowedMimeInput::Bytes {
                        content: &bytes,
                        extent,
                        filename: filename.as_deref(),
                    },
                    &MimeOperationOptions {
                        policy,
                        ..Default::default()
                    },
                )
                .await
        })
        .unwrap();
    match block_on(handle.wait_outcome()).unwrap() {
        qubit_operation::OperationOutcome::Succeeded(value) => value,
        outcome => panic!("repository operation did not succeed: {outcome:?}"),
    }
}

#[test]
fn test_repository_async_candidates_equal_sync() {
    let repository = MimeRepository::bundled();
    let detector: Arc<dyn AsyncMimeDetectorBackend> = Arc::new(RepositoryMimeDetector::with_repository(repository));
    for (input, extent, filename, policy) in [
        (
            Arc::<[u8]>::from(b"%PDF-1.7".as_slice()),
            InputExtent::Complete,
            None,
            qubit_mime::MimeDetectionPolicy::VerifyContent,
        ),
        (
            Arc::<[u8]>::from(b"\xff\xd8\xff\xe0".as_slice()),
            InputExtent::Prefix,
            Some("sample.pdf".to_owned()),
            qubit_mime::MimeDetectionPolicy::ContentFirst,
        ),
        (
            Arc::<[u8]>::from(b"".as_slice()),
            InputExtent::Prefix,
            Some("sample.jpg".to_owned()),
            qubit_mime::MimeDetectionPolicy::PreferFilename,
        ),
    ] {
        let input = input.to_vec();
        let candidates = run_backend(
            Arc::clone(&detector),
            input.clone().into(),
            extent,
            filename.clone(),
            policy,
        );
        let expected = repository
            .detect(filename.as_deref().unwrap_or(""), &input, policy)
            .into_iter()
            .map(|mime| mime.name().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(
            candidates
                .candidates
                .iter()
                .map(|item| item.mime_type.clone())
                .collect::<Vec<_>>(),
            expected
        );
        assert!(matches!(candidates.inspected_extent, InspectedExtent::Prefix(_)));
    }
}

#[test]
fn test_repository_async_yields_and_stops() {
    use std::sync::Mutex;
    use std::task::Poll;

    let mut xml = String::from(r#"<mime-info xmlns="http://www.freedesktop.org/standards/shared-mime-info">"#);
    for index in 0..256 {
        xml.push_str(&format!(
            r#"<mime-type type="application/x-batch-{index}"><magic><match type="byte" value="0x25" offset="0"/></magic></mime-type>"#
        ));
    }
    xml.push_str("</mime-info>");
    let repository = Box::leak(Box::new(MimeRepository::from_xml(&xml).unwrap()));
    let detector: Arc<dyn AsyncMimeDetectorBackend> = Arc::new(RepositoryMimeDetector::with_repository(repository));
    assert_eq!(
        detector.content_requirement(),
        qubit_mime::ContentRequirement::Prefix(repository.max_test_bytes())
    );

    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let group = manager.group(qubit_operation::GroupOptions { capacity: 4 }).unwrap();
    let target_context = Arc::new(Mutex::new(None));
    let target_context_for_body = Arc::clone(&target_context);
    let input: Arc<[u8]> = Arc::from(b"%".as_slice());
    let mut target = group
        .as_async()
        .try_submit(Default::default(), move |scope| async move {
            *target_context_for_body.lock().unwrap() = Some(scope.context());
            detector
                .guess_content_in(
                    &scope,
                    BorrowedMimeInput::Bytes {
                        content: &input,
                        extent: InputExtent::Prefix,
                        filename: None,
                    },
                    &MimeOperationOptions::default(),
                )
                .await
        })
        .unwrap();
    let context_for_heartbeat = Arc::clone(&target_context);
    let mut heartbeat = group
        .as_async()
        .try_submit(Default::default(), move |_| async move {
            let mut ticks = 0usize;
            std::future::poll_fn(|cx| {
                ticks += 1;
                let context = context_for_heartbeat.lock().unwrap().clone();
                if let Some(context) = context {
                    context.request_stop(qubit_operation::StopReason::UserCancelled);
                    Poll::Ready(())
                } else {
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
            })
            .await;
            Ok::<_, qubit_mime::MimeError>(ticks)
        })
        .unwrap();

    let target_outcome = block_on(target.wait_outcome()).unwrap();
    let heartbeat_outcome = block_on(heartbeat.wait_outcome()).unwrap();
    assert!(!matches!(
        target_outcome,
        qubit_operation::OperationOutcome::Succeeded(_)
    ));
    assert!(matches!(heartbeat_outcome, qubit_operation::OperationOutcome::Succeeded(ticks) if ticks > 0));
}
