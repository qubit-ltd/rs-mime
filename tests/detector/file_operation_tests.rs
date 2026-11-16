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
fn file_operation_complete_only_rejects_prefix() {
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
fn file_operation_declares_complete_content_and_no_random_access() {
    let detector = FileCommandMimeDetector::new();
    assert_eq!(detector.content_requirement(), qubit_mime::ContentRequirement::Complete);
    assert_eq!(
        detector.random_access_capability(),
        qubit_mime::RandomAccessCapability::Unsupported
    );
}
