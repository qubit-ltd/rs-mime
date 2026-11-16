use std::future::Future;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};

use qubit_mime::detector::MimeRuntime;
use qubit_mime::{InputExtent, MimeOperationOptions, OwnedMimeInput};

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

#[derive(Debug, Default)]
struct TestExecution {
    prepared: std::sync::atomic::AtomicUsize,
    shutdowns: std::sync::atomic::AtomicUsize,
}

impl qubit_mime::MimeExecutionPort for TestExecution {
    fn prepare(&self) -> Result<(), qubit_mime::MimeError> {
        self.prepared
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }

    fn shutdown(&self) -> Result<(), qubit_mime::MimeError> {
        self.shutdowns
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }
}

fn services(
    manager: &qubit_operation::OperationManager,
    capacity: usize,
    port: Arc<TestExecution>,
) -> qubit_mime::MimeExecutionServices {
    let group = manager
        .group(qubit_operation::GroupOptions { capacity })
        .unwrap();
    qubit_mime::MimeExecutionServices::new(manager.clone(), group, port, 1024).unwrap()
}

#[test]
fn operation_runtime_reject_returns_owned_input() {
    let runtime = MimeRuntime::builtin().unwrap();
    let bytes: Arc<[u8]> = Arc::from(b"%PDF-1.7".as_slice());
    let input = OwnedMimeInput::bytes(Arc::clone(&bytes), InputExtent::Complete);

    let failure = match runtime.try_submit_detection(input, MimeOperationOptions::default()) {
        Ok(_) => panic!("services-free async submission must be rejected"),
        Err(failure) => failure,
    };

    match failure.input {
        OwnedMimeInput::Bytes { content, .. } => assert!(Arc::ptr_eq(&bytes, &content)),
        OwnedMimeInput::Provider { .. } => panic!("bytes input changed kind"),
    }
}

#[test]
fn operation_runtime_owned_budget_limit_returns_input() {
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let port = Arc::new(TestExecution::default());
    let services = services(&manager, 2, Arc::clone(&port));
    services.prepare().unwrap();
    let runtime = MimeRuntime::builtin()
        .unwrap()
        .with_execution_services(services);
    let bytes: Arc<[u8]> = vec![0; 16 * 1024 * 1024 + 1].into();
    let failure = match runtime.try_submit_detection(
        OwnedMimeInput::bytes(Arc::clone(&bytes), InputExtent::Complete),
        MimeOperationOptions::default(),
    ) {
        Ok(_) => panic!("oversized input unexpectedly entered MIME execution"),
        Err(failure) => failure,
    };

    assert!(matches!(
        failure.reason,
        qubit_mime::MimeSubmitRejection::BudgetExceeded
    ));
    match failure.input {
        OwnedMimeInput::Bytes { content, .. } => assert!(Arc::ptr_eq(&bytes, &content)),
        OwnedMimeInput::Provider { .. } => panic!("bytes input changed kind"),
    }
}

#[test]
fn operation_runtime_group_shutdown_keeps_other_group() {
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let port = Arc::new(TestExecution::default());
    let services = services(&manager, 2, Arc::clone(&port));
    services.prepare().unwrap();
    let first = services.shutdown();
    let repeated = services.shutdown();
    assert!(
        block_on(first.wait_services_settled())
            .unwrap()
            .operations_settled
    );
    assert!(first.stop_services().unwrap().services_stopped);
    assert!(block_on(first.wait_group_shutdown()).is_ok());
    assert!(
        block_on(repeated.wait_services_settled())
            .unwrap()
            .services_stopped
    );
    assert!(repeated.stop_services().unwrap().services_stopped);
    assert!(block_on(repeated.wait_group_shutdown()).is_ok());
    assert_eq!(port.prepared.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(port.shutdowns.load(std::sync::atomic::Ordering::SeqCst), 1);

    let side_group = manager
        .group(qubit_operation::GroupOptions { capacity: 2 })
        .unwrap();
    assert!(
        side_group
            .try_submit(Default::default(), |_| Ok::<_, ()>(()))
            .is_ok()
    );
}

#[test]
fn operation_runtime_shutdown_waits_for_accepted_work() {
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let port = Arc::new(TestExecution::default());
    let services = services(&manager, 2, Arc::clone(&port));
    services.prepare().unwrap();
    let runtime = MimeRuntime::builtin()
        .unwrap()
        .with_execution_services(services.clone());
    let mut handle = match runtime.try_submit_detection(
        OwnedMimeInput::bytes(Arc::from(b"%PDF-1.7".as_slice()), InputExtent::Complete),
        MimeOperationOptions::default(),
    ) {
        Ok(handle) => handle,
        Err(_) => panic!("prepared runtime should accept the operation"),
    };

    let shutdown = services.shutdown();
    let failure = runtime.try_submit_detection(
        OwnedMimeInput::bytes(Arc::from(b"%PDF".as_slice()), InputExtent::Complete),
        MimeOperationOptions::default(),
    );
    let failure = match failure {
        Ok(_) => panic!("shutdown runtime accepted a new root request"),
        Err(failure) => failure,
    };
    assert!(matches!(
        failure.reason,
        qubit_mime::MimeSubmitRejection::ExecutionServicesRequired
    ));

    let settled = block_on(shutdown.wait_services_settled()).unwrap();
    assert!(settled.operations_settled);
    assert!(!settled.services_stopped);
    let stopped = shutdown.stop_services().unwrap();
    assert!(stopped.services_stopped);
    let group = stopped
        .group
        .expect("Drain was requested after service settlement");
    assert!(
        group.active_operations > 0,
        "typed handle keeps group shutdown observable as pending"
    );
    assert_eq!(port.shutdowns.load(std::sync::atomic::Ordering::SeqCst), 1);

    let _outcome = block_on(handle.wait_outcome()).unwrap();
    let complete = block_on(shutdown.wait_group_shutdown()).unwrap();
    assert_eq!(complete.snapshot.active_operations, 0);
}

#[test]
fn operation_runtime_child_does_not_reserve_root() {
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let port = Arc::new(TestExecution::default());
    let services = services(&manager, 2, Arc::clone(&port));
    services.prepare().unwrap();
    let runtime = MimeRuntime::builtin()
        .unwrap()
        .with_execution_services(services.clone());
    let child_slot = Arc::new(std::sync::Mutex::new(None));
    let child_slot_body = Arc::clone(&child_slot);
    let child_runtime = runtime.clone();
    let mut parent = manager
        .try_submit(Default::default(), move |scope| {
            let child = child_runtime.try_submit_detection_in(
                &scope,
                OwnedMimeInput::bytes(Arc::from(b"%PDF-1.7".as_slice()), InputExtent::Complete),
                MimeOperationOptions::default(),
            );
            match child {
                Ok(child) => {
                    *child_slot_body.lock().unwrap() = Some(child);
                    Ok::<_, qubit_mime::MimeError>(true)
                }
                Err(_) => Ok(false),
            }
        })
        .unwrap();

    assert!(matches!(
        parent.wait_outcome().unwrap(),
        qubit_operation::OperationOutcome::Succeeded(true)
    ));
    let mut child = child_slot
        .lock()
        .unwrap()
        .take()
        .expect("explicit parent binding should admit the child");
    assert!(matches!(
        block_on(child.wait_outcome()).unwrap(),
        qubit_operation::OperationOutcome::Succeeded(_)
    ));

    let shutdown = services.shutdown();
    block_on(shutdown.wait_services_settled()).unwrap();
    shutdown.stop_services().unwrap();
    let _report = block_on(shutdown.wait_group_shutdown()).unwrap();
}

#[test]
fn operation_runtime_queue_and_budget_boundaries() {
    use std::sync::mpsc;

    let mut manager_options = qubit_operation::ManagerOptions::default();
    manager_options.limits.active_operations = 2;
    manager_options.limits.queued_operations = 1;
    manager_options.async_executor.worker_count = 1;
    let manager = qubit_operation::OperationManager::new(manager_options).unwrap();
    let port = Arc::new(TestExecution::default());
    let group = manager
        .group(qubit_operation::GroupOptions { capacity: 4 })
        .unwrap();
    let services =
        qubit_mime::MimeExecutionServices::new(manager.clone(), group.clone(), port, 1024).unwrap();
    services.prepare().unwrap();
    let runtime = MimeRuntime::builtin()
        .unwrap()
        .with_execution_services(services.clone());

    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let mut active = group
        .as_async()
        .try_submit(Default::default(), move |_| async move {
            entered_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            Ok::<_, qubit_mime::MimeError>(())
        })
        .unwrap();
    entered_rx.recv().unwrap();
    assert_eq!(group.snapshot().active_operations, 1);

    let queued = match runtime.try_submit_detection(
        OwnedMimeInput::bytes(Arc::from(b"%PDF-queued".as_slice()), InputExtent::Complete),
        MimeOperationOptions::default(),
    ) {
        Ok(handle) => handle,
        Err(_) => panic!("one operation should fit in the bounded queue"),
    };
    assert_eq!(
        queued.observer().snapshot().execution,
        qubit_operation::ExecutionPhase::Queued
    );

    let bytes: Arc<[u8]> = Arc::from(b"%PDF-rejected".as_slice());
    let failure = match runtime.try_submit_detection(
        OwnedMimeInput::bytes(Arc::clone(&bytes), InputExtent::Complete),
        MimeOperationOptions::default(),
    ) {
        Ok(_) => panic!("operation beyond active plus queue capacity was admitted"),
        Err(failure) => failure,
    };
    assert!(matches!(
        failure.reason,
        qubit_mime::MimeSubmitRejection::Admission(qubit_operation::AdmissionError::Busy)
    ));
    match failure.input {
        OwnedMimeInput::Bytes { content, .. } => assert!(Arc::ptr_eq(&bytes, &content)),
        OwnedMimeInput::Provider { .. } => panic!("bytes input changed kind"),
    }

    release_tx.send(()).unwrap();
    let _active_outcome = block_on(active.wait_outcome()).unwrap();
    let mut queued = queued;
    let _queued_outcome = block_on(queued.wait_outcome()).unwrap();
    let mut retry = runtime
        .try_submit_detection(
            OwnedMimeInput::bytes(Arc::clone(&bytes), InputExtent::Complete),
            MimeOperationOptions::default(),
        )
        .unwrap_or_else(|_| panic!("rejected admission must release its owned-byte permit"));
    let _retry_outcome = block_on(retry.wait_outcome()).unwrap();
    let shutdown = services.shutdown();
    block_on(shutdown.wait_services_settled()).unwrap();
    shutdown.stop_services().unwrap();
    let _report = block_on(shutdown.wait_group_shutdown()).unwrap();
}

#[test]
fn operation_runtime_wrong_manager_returns_original_input() {
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let foreign = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let port = Arc::new(TestExecution::default());
    let services = services(&manager, 2, port);
    services.prepare().unwrap();
    let runtime = MimeRuntime::builtin()
        .unwrap()
        .with_execution_services(services.clone());
    let bytes: Arc<[u8]> = Arc::from(b"wrong-manager".as_slice());
    let body_runtime = runtime.clone();
    let body_bytes = Arc::clone(&bytes);
    let mut parent = foreign
        .try_submit(Default::default(), move |scope| {
            let failure = match body_runtime.try_submit_detection_in(
                &scope,
                OwnedMimeInput::bytes(Arc::clone(&body_bytes), InputExtent::Complete),
                MimeOperationOptions::default(),
            ) {
                Ok(_) => return Ok(false),
                Err(failure) => failure,
            };
            let correct_reason = matches!(
                failure.reason,
                qubit_mime::MimeSubmitRejection::GroupBinding(
                    qubit_operation::GroupBindingError::WrongManager
                )
            );
            let same_input = match failure.input {
                OwnedMimeInput::Bytes { content, .. } => Arc::ptr_eq(&body_bytes, &content),
                OwnedMimeInput::Provider { .. } => false,
            };
            Ok::<_, ()>(correct_reason && same_input)
        })
        .unwrap();
    assert!(matches!(
        parent.wait_outcome().unwrap(),
        qubit_operation::OperationOutcome::Succeeded(true)
    ));
    let shutdown = services.shutdown();
    block_on(shutdown.wait_services_settled()).unwrap();
    shutdown.stop_services().unwrap();
    let _report = block_on(shutdown.wait_group_shutdown()).unwrap();
}

#[test]
fn operation_runtime_ended_parent_returns_original_input() {
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let port = Arc::new(TestExecution::default());
    let services = services(&manager, 2, port);
    services.prepare().unwrap();
    let runtime = MimeRuntime::builtin()
        .unwrap()
        .with_execution_services(services.clone());
    let saved_scope = Arc::new(std::sync::Mutex::new(None));
    let body_scope = Arc::clone(&saved_scope);
    let mut parent = manager
        .try_submit(Default::default(), move |scope| {
            *body_scope.lock().unwrap() = Some(scope);
            Ok::<_, ()>(())
        })
        .unwrap();
    let _parent_outcome = parent.wait_outcome().unwrap();
    let scope = saved_scope.lock().unwrap().take().unwrap();
    let bytes: Arc<[u8]> = Arc::from(b"ended-parent".as_slice());
    let failure = match runtime.try_submit_detection_in(
        &scope,
        OwnedMimeInput::bytes(Arc::clone(&bytes), InputExtent::Complete),
        MimeOperationOptions::default(),
    ) {
        Ok(_) => panic!("ended parent scope accepted a child"),
        Err(failure) => failure,
    };
    assert!(matches!(
        failure.reason,
        qubit_mime::MimeSubmitRejection::GroupBinding(
            qubit_operation::GroupBindingError::ScopeEnded
        )
    ));
    match failure.input {
        OwnedMimeInput::Bytes { content, .. } => assert!(Arc::ptr_eq(&bytes, &content)),
        OwnedMimeInput::Provider { .. } => panic!("bytes input changed kind"),
    }
    let shutdown = services.shutdown();
    block_on(shutdown.wait_services_settled()).unwrap();
    shutdown.stop_services().unwrap();
    let _report = block_on(shutdown.wait_group_shutdown()).unwrap();
}

#[test]
fn operation_runtime_sync_submission_uses_parent() {
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let port = Arc::new(TestExecution::default());
    let services = services(&manager, 2, port);
    services.prepare().unwrap();
    let runtime = MimeRuntime::builtin()
        .unwrap()
        .with_execution_services(services.clone());
    let child_slot = Arc::new(std::sync::Mutex::new(None));
    let body_slot = Arc::clone(&child_slot);
    let body_runtime = runtime.clone();
    let mut parent = manager
        .try_submit(Default::default(), move |scope| {
            let child = body_runtime.try_submit_detection_sync_in(
                &scope,
                OwnedMimeInput::bytes(Arc::from(b"%PDF-1.7".as_slice()), InputExtent::Complete),
                MimeOperationOptions::default(),
            );
            match child {
                Ok(child) => {
                    *body_slot.lock().unwrap() = Some(child);
                    Ok::<_, ()>(true)
                }
                Err(_) => Ok(false),
            }
        })
        .unwrap();
    assert!(matches!(
        parent.wait_outcome().unwrap(),
        qubit_operation::OperationOutcome::Succeeded(true)
    ));
    let mut child = child_slot
        .lock()
        .unwrap()
        .take()
        .expect("sync child should be accepted");
    assert!(matches!(
        child.wait_outcome().unwrap(),
        qubit_operation::OperationOutcome::Succeeded(_)
    ));
    let shutdown = services.shutdown();
    block_on(shutdown.wait_services_settled()).unwrap();
    shutdown.stop_services().unwrap();
    let _report = block_on(shutdown.wait_group_shutdown()).unwrap();
}

#[test]
fn operation_runtime_closed_group_returns_original_input() {
    use std::sync::mpsc;

    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let port = Arc::new(TestExecution::default());
    let services = services(&manager, 2, port);
    services.prepare().unwrap();
    let runtime = MimeRuntime::builtin()
        .unwrap()
        .with_execution_services(services.clone());
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let body_runtime = runtime.clone();
    let mut parent = manager
        .try_submit(Default::default(), move |scope| {
            entered_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            let bytes: Arc<[u8]> = Arc::from(b"closed-group".as_slice());
            let failure = match body_runtime.try_submit_detection_in(
                &scope,
                OwnedMimeInput::bytes(Arc::clone(&bytes), InputExtent::Complete),
                MimeOperationOptions::default(),
            ) {
                Ok(_) => return Ok(false),
                Err(failure) => failure,
            };
            let group_closed = matches!(
                failure.reason,
                qubit_mime::MimeSubmitRejection::GroupBinding(
                    qubit_operation::GroupBindingError::Closed
                )
            );
            let same_input = match failure.input {
                OwnedMimeInput::Bytes { content, .. } => Arc::ptr_eq(&bytes, &content),
                OwnedMimeInput::Provider { .. } => false,
            };
            Ok::<_, ()>(group_closed && same_input)
        })
        .unwrap();
    entered_rx.recv().unwrap();
    let shutdown = services.shutdown();
    block_on(shutdown.wait_services_settled()).unwrap();
    shutdown.stop_services().unwrap();
    release_tx.send(()).unwrap();
    assert!(matches!(
        parent.wait_outcome().unwrap(),
        qubit_operation::OperationOutcome::Succeeded(true)
    ));
    let _report = block_on(shutdown.wait_group_shutdown()).unwrap();
}
