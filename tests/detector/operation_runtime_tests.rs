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
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::task::Context;
use std::task::Poll;
use std::task::Wake;
use std::task::Waker;

use qubit_mime::InputExtent;
use qubit_mime::MimeOperationOptions;
use qubit_mime::OwnedMimeInput;
use qubit_mime::detector::MimeRuntime;

#[derive(Debug)]
struct CountingFileSystem {
    properties: qubit_fs::spi::ProviderProperties,
    opens: Arc<AtomicUsize>,
}

impl CountingFileSystem {
    fn new(opens: Arc<AtomicUsize>) -> Self {
        use qubit_fs::metadata::FileSystemCapabilities;
        use qubit_fs::metadata::FileSystemCapability;
        use qubit_fs::metadata::FileSystemId;
        use qubit_fs::metadata::FileSystemInfo;
        use qubit_fs::metadata::FileSystemLimits;
        use qubit_fs::metadata::SymlinkPolicy;
        use qubit_fs::path::PathConstraints;
        use qubit_fs::path::PathSemantics;
        use qubit_fs::spi::ProviderOperation;
        use qubit_fs::spi::ProviderOperations;
        use qubit_fs::spi::ProviderProperties;
        Self {
            properties: ProviderProperties::new(
                FileSystemInfo::new(
                    FileSystemId::new("mime-runtime-counting").unwrap(),
                    "mime-runtime-counting",
                    PathSemantics::Hierarchical,
                ),
                ProviderOperations::new().with(ProviderOperation::OpenReader),
                FileSystemCapabilities::new().with_guaranteed(FileSystemCapability::Read),
                FileSystemLimits::unknown(),
                PathConstraints::absolute(),
                SymlinkPolicy::Reject,
            )
            .unwrap(),
            opens,
        }
    }
}

impl qubit_fs::spi::AsyncFileSystemSpi for CountingFileSystem {
    fn properties(&self) -> qubit_fs::spi::ProviderProperties {
        self.properties.clone()
    }

    fn stat<'a>(
        &'a self,
        _request: qubit_fs::spi::StatRequest<'a>,
    ) -> qubit_fs::spi::SpiFuture<'a, qubit_fs::FsResult<qubit_fs::spi::StatResponse>> {
        Box::pin(async { panic!("stat should not be called") })
    }

    fn open_reader<'a>(
        &'a self,
        request: qubit_fs::spi::OpenReaderRequest<'a>,
    ) -> qubit_fs::spi::SpiFuture<'a, qubit_fs::FsResult<qubit_fs::spi::OpenedAsyncReader>> {
        self.opens.fetch_add(1, Ordering::SeqCst);
        let info = qubit_fs::metadata::OpenedFileInfo::new(
            qubit_fs::metadata::FileSystemId::new("mime-runtime-counting").unwrap(),
            request.path().clone(),
        );
        Box::pin(async move {
            Ok(qubit_fs::spi::OpenedAsyncReader::new(
                info,
                Box::new(PdfInput {
                    bytes: b"%PDF".to_vec(),
                    position: 0,
                }),
            ))
        })
    }
}

struct PdfInput {
    bytes: Vec<u8>,
    position: usize,
}

impl qubit_io::AsyncInput for PdfInput {
    type Item = u8;
    unsafe fn poll_read_unchecked(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        output: &mut [u8],
        index: usize,
        count: usize,
    ) -> Poll<std::io::Result<usize>> {
        let read = count.min(self.bytes.len().saturating_sub(self.position));
        output[index..index + read].copy_from_slice(&self.bytes[self.position..self.position + read]);
        self.position += read;
        Poll::Ready(Ok(read))
    }
}

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
        self.prepared.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }

    fn shutdown(&self) -> Result<(), qubit_mime::MimeError> {
        self.shutdowns.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }
}

fn services(
    manager: &qubit_operation::OperationManager,
    capacity: usize,
    port: Arc<TestExecution>,
) -> qubit_mime::MimeExecutionServices {
    let group = manager.group(qubit_operation::GroupOptions { capacity }).unwrap();
    qubit_mime::MimeExecutionServices::new(manager.clone(), group, port, 1024).unwrap()
}

#[test]
fn test_operation_runtime_reject_returns_owned_input() {
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
fn test_operation_runtime_owned_budget_limit_returns_input() {
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let port = Arc::new(TestExecution::default());
    let services = services(&manager, 2, Arc::clone(&port));
    services.prepare().unwrap();
    let runtime = MimeRuntime::builtin().unwrap().with_execution_services(services);
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
fn test_operation_runtime_group_shutdown_keeps_other_group() {
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let port = Arc::new(TestExecution::default());
    let services = services(&manager, 2, Arc::clone(&port));
    services.prepare().unwrap();
    let first = services.shutdown();
    let repeated = services.shutdown();
    assert!(block_on(first.wait_services_settled()).unwrap().operations_settled);
    assert!(first.stop_services().unwrap().services_stopped);
    assert!(block_on(first.wait_group_shutdown()).is_ok());
    assert!(block_on(repeated.wait_services_settled()).unwrap().services_stopped);
    assert!(repeated.stop_services().unwrap().services_stopped);
    assert!(block_on(repeated.wait_group_shutdown()).is_ok());
    assert_eq!(port.prepared.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(port.shutdowns.load(std::sync::atomic::Ordering::SeqCst), 1);

    let side_group = manager.group(qubit_operation::GroupOptions { capacity: 2 }).unwrap();
    assert!(side_group.try_submit(Default::default(), |_| Ok::<_, ()>(())).is_ok());
}

#[test]
fn test_operation_runtime_prepare_retries_after_failure_and_rejects_after_shutdown() {
    #[derive(Debug, Default)]
    struct RetryPreparePort {
        prepares: AtomicUsize,
        shutdowns: AtomicUsize,
    }

    impl qubit_mime::MimeExecutionPort for RetryPreparePort {
        fn prepare(&self) -> Result<(), qubit_mime::MimeError> {
            if self.prepares.fetch_add(1, Ordering::SeqCst) == 0 {
                Err(qubit_mime::MimeError::InvalidOperationBudget)
            } else {
                Ok(())
            }
        }

        fn shutdown(&self) -> Result<(), qubit_mime::MimeError> {
            self.shutdowns.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let group = manager.group(qubit_operation::GroupOptions { capacity: 2 }).unwrap();
    let port = Arc::new(RetryPreparePort::default());
    let execution: Arc<dyn qubit_mime::MimeExecutionPort> = port.clone();
    let services = qubit_mime::MimeExecutionServices::new(manager, group, execution, 1024).unwrap();

    assert!(matches!(
        services.prepare(),
        Err(qubit_mime::MimeError::InvalidOperationBudget)
    ));
    services
        .prepare()
        .expect("prepare should retry after the failed attempt");
    services.prepare().expect("prepare should be idempotent while running");
    assert_eq!(2, port.prepares.load(Ordering::SeqCst));

    let shutdown = services.shutdown();
    assert!(block_on(shutdown.wait_services_settled()).unwrap().operations_settled);
    shutdown.stop_services().unwrap();
    assert!(matches!(
        services.prepare(),
        Err(qubit_mime::MimeError::ExecutionServicesRequired)
    ));
    assert_eq!(1, port.shutdowns.load(Ordering::SeqCst));
    let _report = block_on(shutdown.wait_group_shutdown()).unwrap();
}

#[test]
fn test_operation_runtime_releases_owned_budget_after_operation_settles() {
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let port = Arc::new(TestExecution::default());
    let services = services(&manager, 2, Arc::clone(&port));
    services.prepare().unwrap();
    let runtime = MimeRuntime::builtin()
        .unwrap()
        .with_execution_services(services.clone());
    let input_size = 12 * 1024 * 1024;

    for _ in 0..2 {
        let bytes: Arc<[u8]> = vec![0; input_size].into();
        let mut handle = match runtime.try_submit_detection(
            OwnedMimeInput::bytes(bytes, InputExtent::Complete),
            MimeOperationOptions::default(),
        ) {
            Ok(handle) => handle,
            Err(_) => panic!("settled operation should release its owned-memory reservation"),
        };
        let _outcome = block_on(handle.wait_outcome()).expect("accepted operation should settle");
    }

    let shutdown = services.shutdown();
    block_on(shutdown.wait_services_settled()).unwrap();
    shutdown.stop_services().unwrap();
    let _report = block_on(shutdown.wait_group_shutdown()).unwrap();
}

#[test]
fn test_operation_runtime_shutdown_waits_for_accepted_work() {
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
    let group = stopped.group.expect("Drain was requested after service settlement");
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
fn test_operation_runtime_child_does_not_reserve_root() {
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
fn test_operation_runtime_queue_and_budget_boundaries() {
    use std::sync::mpsc;

    let mut manager_options = qubit_operation::ManagerOptions::default();
    manager_options.limits.active_operations = 2;
    manager_options.limits.queued_operations = 1;
    manager_options.async_executor.worker_count = 1;
    let manager = qubit_operation::OperationManager::new(manager_options).unwrap();
    let port = Arc::new(TestExecution::default());
    let group = manager.group(qubit_operation::GroupOptions { capacity: 4 }).unwrap();
    let services = qubit_mime::MimeExecutionServices::new(manager.clone(), group.clone(), port, 1024).unwrap();
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
fn test_operation_runtime_wrong_manager_returns_original_input() {
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
                qubit_mime::MimeSubmitRejection::GroupBinding(qubit_operation::GroupBindingError::WrongManager)
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
fn test_operation_runtime_ended_parent_returns_original_input() {
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
        qubit_mime::MimeSubmitRejection::GroupBinding(qubit_operation::GroupBindingError::ScopeEnded)
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
fn test_operation_runtime_sync_submission_uses_parent() {
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
    match child.wait_outcome().unwrap() {
        qubit_operation::OperationOutcome::Succeeded(report) => assert_eq!(report.budget.owned_bytes, 8),
        other => panic!("sync child should succeed: {other:?}"),
    }
    let shutdown = services.shutdown();
    block_on(shutdown.wait_services_settled()).unwrap();
    shutdown.stop_services().unwrap();
    let _report = block_on(shutdown.wait_group_shutdown()).unwrap();
}

#[test]
fn test_operation_runtime_sync_and_async_reports_match_repository_inspection_extent() {
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let port = Arc::new(TestExecution::default());
    let services = services(&manager, 8, port);
    services.prepare().unwrap();
    let runtime = MimeRuntime::builtin()
        .unwrap()
        .with_execution_services(services.clone());
    let max_test_bytes = qubit_mime::MimeRepository::bundled().max_test_bytes();
    let mut bytes = vec![b'x'; max_test_bytes + 1];
    bytes[..8].copy_from_slice(b"%PDF-1.7");
    let bytes: Arc<[u8]> = Arc::from(bytes);
    let handles = Arc::new(std::sync::Mutex::new(Vec::new()));
    let body_handles = Arc::clone(&handles);
    let body_runtime = runtime.clone();
    let body_bytes = Arc::clone(&bytes);
    let mut parent = manager
        .try_submit(Default::default(), move |scope| {
            let cases = [
                (InputExtent::Complete, qubit_mime::MimeDetectionPolicy::VerifyContent, "extent.pdf"),
                (InputExtent::Prefix, qubit_mime::MimeDetectionPolicy::VerifyContent, "extent.pdf"),
                (InputExtent::Complete, qubit_mime::MimeDetectionPolicy::PreferFilename, "unique-name.pdf"),
            ];
            let mut accepted = true;
            for (extent, policy, filename) in cases {
                let options = MimeOperationOptions {
                    policy,
                    ..Default::default()
                };
                let sync = body_runtime.try_submit_detection_sync_in(
                    &scope,
                    OwnedMimeInput::bytes(Arc::clone(&body_bytes), extent).with_filename(filename),
                    options.clone(),
                );
                let asynchronous = body_runtime.try_submit_detection_in(
                    &scope,
                    OwnedMimeInput::bytes(Arc::clone(&body_bytes), extent).with_filename(filename),
                    options,
                );
                match (sync, asynchronous) {
                    (Ok(sync), Ok(asynchronous)) => body_handles.lock().unwrap().push((sync, asynchronous)),
                    _ => accepted = false,
                }
            }
            Ok::<_, ()>(accepted)
        })
        .unwrap();
    assert!(matches!(
        parent.wait_outcome().unwrap(),
        qubit_operation::OperationOutcome::Succeeded(true)
    ));

    let pairs = std::mem::take(&mut *handles.lock().unwrap());
    assert_eq!(3, pairs.len());
    for (index, (mut sync, mut asynchronous)) in pairs.into_iter().enumerate() {
        let sync_report = match sync.wait_outcome().unwrap() {
            qubit_operation::OperationOutcome::Succeeded(report) => report,
            other => panic!("sync operation should succeed: {other:?}"),
        };
        let async_report = match block_on(asynchronous.wait_outcome()).unwrap() {
            qubit_operation::OperationOutcome::Succeeded(report) => report,
            other => panic!("async operation should succeed: {other:?}"),
        };
        assert_eq!(Some("application/pdf".to_owned()), sync_report.mime_type);
        assert_eq!(sync_report.mime_type, async_report.mime_type);
        match index {
            0 | 1 => assert_eq!(
                qubit_mime::InspectedExtent::Prefix(max_test_bytes),
                sync_report.inspected_extent,
                "repository matching inspects only its bounded magic window"
            ),
            2 => assert_eq!(
                qubit_mime::InspectedExtent::Prefix(0),
                sync_report.inspected_extent,
                "PreferFilename returns before repository content matching"
            ),
            _ => unreachable!(),
        }
        assert_eq!(
            qubit_mime::InspectedExtent::Prefix(max_test_bytes),
            async_report.inspected_extent,
            "async repository backend reports its bounded matcher window"
        );
    }

    let shutdown = services.shutdown();
    block_on(shutdown.wait_services_settled()).unwrap();
    shutdown.stop_services().unwrap();
    let _report = block_on(shutdown.wait_group_shutdown()).unwrap();
}

#[test]
fn test_operation_runtime_sync_rejects_provider_and_returns_original_input() {
    use qubit_fs::AsyncFileSystem;
    use qubit_fs::Path;

    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let port = Arc::new(TestExecution::default());
    let services = services(&manager, 2, port);
    services.prepare().unwrap();
    let runtime = MimeRuntime::builtin()
        .unwrap()
        .with_execution_services(services.clone());
    let opens = Arc::new(AtomicUsize::new(0));
    let file_system = Arc::new(AsyncFileSystem::from_spi(CountingFileSystem::new(Arc::clone(&opens))).unwrap());
    let expected_file_system = Arc::clone(&file_system);
    let expected_path = Path::parse("/document.pdf").unwrap();
    let body_runtime = runtime.clone();
    let mut parent = manager
        .try_submit(Default::default(), move |scope| {
            let input = OwnedMimeInput::Provider {
                file_system,
                path: expected_path.clone(),
            };
            let matches =
                match body_runtime.try_submit_detection_sync_in(&scope, input, MimeOperationOptions::default()) {
                    Ok(_) => false,
                    Err(failure) => {
                        let unsupported = matches!(failure.reason, qubit_mime::MimeSubmitRejection::AsyncUnsupported);
                        let input_returned = match failure.input {
                            OwnedMimeInput::Provider { file_system, path } => {
                                Arc::ptr_eq(&expected_file_system, &file_system) && path == expected_path
                            }
                            OwnedMimeInput::Bytes { .. } => false,
                        };
                        unsupported && input_returned
                    }
                };
            Ok::<_, ()>(matches)
        })
        .unwrap();

    assert!(matches!(
        parent.wait_outcome().unwrap(),
        qubit_operation::OperationOutcome::Succeeded(true)
    ));
    assert_eq!(
        0,
        opens.load(Ordering::SeqCst),
        "rejected provider input must not be read"
    );
    let shutdown = services.shutdown();
    block_on(shutdown.wait_services_settled()).unwrap();
    shutdown.stop_services().unwrap();
    let _report = block_on(shutdown.wait_group_shutdown()).unwrap();
}

#[test]
fn test_operation_runtime_closed_group_returns_original_input() {
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
                qubit_mime::MimeSubmitRejection::GroupBinding(qubit_operation::GroupBindingError::Closed)
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

#[test]
fn test_operation_runtime_sync_preflight_matches_async() {
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let port = Arc::new(TestExecution::default());
    let services = services(&manager, 4, port);
    services.prepare().unwrap();
    let runtime = MimeRuntime::builtin()
        .unwrap()
        .with_execution_services(services.clone());
    let body_runtime = runtime.clone();
    let mut parent = manager
        .try_submit(Default::default(), move |scope| {
            let zero_bytes: Arc<[u8]> = Arc::from(b"zero-read".as_slice());
            let zero_options = MimeOperationOptions { max_read_bytes: Some(0), ..Default::default() };
            let sync_zero = body_runtime.try_submit_detection_sync_in(
                &scope,
                OwnedMimeInput::bytes(Arc::clone(&zero_bytes), InputExtent::Complete),
                zero_options.clone(),
            );
            let async_zero = body_runtime.try_submit_detection_in(
                &scope,
                OwnedMimeInput::bytes(Arc::clone(&zero_bytes), InputExtent::Complete),
                zero_options,
            );
            let zero_matches = matches!(sync_zero, Err(ref failure) if matches!(failure.reason, qubit_mime::MimeSubmitRejection::InvalidOptions(_)))
                && matches!(async_zero, Err(ref failure) if matches!(failure.reason, qubit_mime::MimeSubmitRejection::InvalidOptions(_)))
                && returned_same_bytes(sync_zero, &zero_bytes)
                && returned_same_async_bytes(async_zero, &zero_bytes);

            let limited_bytes: Arc<[u8]> = Arc::from(b"buffer-limit".as_slice());
            let limited_options = MimeOperationOptions { max_buffer_bytes: Some(2), ..Default::default() };
            let sync_limited = body_runtime.try_submit_detection_sync_in(
                &scope,
                OwnedMimeInput::bytes(Arc::clone(&limited_bytes), InputExtent::Complete),
                limited_options.clone(),
            );
            let async_limited = body_runtime.try_submit_detection_in(
                &scope,
                OwnedMimeInput::bytes(Arc::clone(&limited_bytes), InputExtent::Complete),
                limited_options,
            );
            let limit_matches = matches!(sync_limited, Err(ref failure) if matches!(failure.reason, qubit_mime::MimeSubmitRejection::BudgetExceeded))
                && matches!(async_limited, Err(ref failure) if matches!(failure.reason, qubit_mime::MimeSubmitRejection::BudgetExceeded))
                && returned_same_bytes(sync_limited, &limited_bytes)
                && returned_same_async_bytes(async_limited, &limited_bytes);
            Ok::<_, ()>(zero_matches && limit_matches)
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
fn test_operation_runtime_parallel_children_share_actual_read_budget() {
    use qubit_fs::AsyncFileSystem;
    use qubit_fs::Path;

    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let port = Arc::new(TestExecution::default());
    let group = manager.group(qubit_operation::GroupOptions { capacity: 4 }).unwrap();
    let services = qubit_mime::MimeExecutionServices::new(manager.clone(), group, port, 6).unwrap();
    services.prepare().unwrap();
    let runtime = MimeRuntime::builtin()
        .unwrap()
        .with_execution_services(services.clone());
    let opens = Arc::new(AtomicUsize::new(0));
    let opens_body = Arc::clone(&opens);
    let child_slot = Arc::new(std::sync::Mutex::new(Vec::new()));
    let child_slot_body = Arc::clone(&child_slot);
    let body_runtime = runtime.clone();
    let parent_options = qubit_operation::OperationOptions {
        child_failure_policy: qubit_operation::ChildFailurePolicy::ContinueSiblings,
        ..Default::default()
    };
    let mut parent = manager
        .try_submit(parent_options, move |scope| {
            let mut accepted = true;
            for _ in 0..2 {
                let fs = Arc::new(AsyncFileSystem::from_spi(CountingFileSystem::new(Arc::clone(&opens_body))).unwrap());
                let input = OwnedMimeInput::Provider {
                    file_system: fs,
                    path: Path::parse("/document.pdf").unwrap(),
                };
                let options = MimeOperationOptions {
                    max_buffer_bytes: Some(4),
                    ..Default::default()
                };
                match body_runtime.try_submit_detection_in(&scope, input, options) {
                    Ok(child) => child_slot_body.lock().unwrap().push(child),
                    Err(_) => {
                        accepted = false;
                    }
                }
            }
            Ok::<_, ()>(accepted)
        })
        .unwrap();
    let _parent_outcome = parent.wait_outcome().unwrap();

    let children = std::mem::take(&mut *child_slot.lock().unwrap());
    assert_eq!(children.len(), 2);
    let outcomes: Vec<_> = children
        .into_iter()
        .map(|mut child| block_on(child.wait_outcome()).unwrap())
        .collect();
    let reports: Vec<_> = outcomes
        .iter()
        .filter_map(|outcome| match outcome {
            qubit_operation::OperationOutcome::Succeeded(report) => Some(report),
            _ => None,
        })
        .collect();
    let read_failures = outcomes
        .iter()
        .filter(|outcome| {
            matches!(
                outcome,
                qubit_operation::OperationOutcome::Failed(qubit_mime::MimeError::BudgetExceeded { kind: "read", .. })
            )
        })
        .count();
    assert_eq!(reports.len(), 1);
    assert_eq!(read_failures, 1);
    assert_eq!(reports[0].budget.bytes_read, 4);
    assert_eq!(reports[0].budget.owned_bytes, 4);
    assert_eq!(
        opens.load(Ordering::SeqCst),
        1,
        "the rejected cumulative read must not reach the provider"
    );

    let shutdown = services.shutdown();
    block_on(shutdown.wait_services_settled()).unwrap();
    shutdown.stop_services().unwrap();
    let _report = block_on(shutdown.wait_group_shutdown()).unwrap();
}

#[test]
fn test_operation_runtime_independent_roots_have_independent_read_limits() {
    use qubit_fs::AsyncFileSystem;
    use qubit_fs::Path;

    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let port = Arc::new(TestExecution::default());
    let services = services(&manager, 4, port);
    services.prepare().unwrap();
    let runtime = MimeRuntime::builtin()
        .unwrap()
        .with_execution_services(services.clone());
    let opens = Arc::new(AtomicUsize::new(0));
    let mut handles = Vec::new();
    for _ in 0..2 {
        let fs = Arc::new(AsyncFileSystem::from_spi(CountingFileSystem::new(Arc::clone(&opens))).unwrap());
        let input = OwnedMimeInput::Provider {
            file_system: fs,
            path: Path::parse("/document.pdf").unwrap(),
        };
        let options = MimeOperationOptions {
            max_buffer_bytes: Some(4),
            ..Default::default()
        };
        handles.push(match runtime.try_submit_detection(input, options) {
            Ok(handle) => handle,
            Err(_) => panic!("prepared runtime should admit independent read roots"),
        });
    }
    for mut handle in handles {
        let report = match block_on(handle.wait_outcome()).unwrap() {
            qubit_operation::OperationOutcome::Succeeded(report) => report,
            outcome => panic!("independent root should fit its own read budget: {outcome:?}"),
        };
        assert_eq!(report.budget.bytes_read, 4);
        assert_eq!(report.budget.owned_bytes, 4);
    }
    assert_eq!(opens.load(Ordering::SeqCst), 2);
    let shutdown = services.shutdown();
    block_on(shutdown.wait_services_settled()).unwrap();
    shutdown.stop_services().unwrap();
    let _report = block_on(shutdown.wait_group_shutdown()).unwrap();
}

fn returned_same_bytes(
    result: Result<qubit_mime::MimeOperation, qubit_mime::MimeSubmitFailure>,
    original: &Arc<[u8]>,
) -> bool {
    match result {
        Err(failure) => match failure.input {
            OwnedMimeInput::Bytes { content, .. } => Arc::ptr_eq(original, &content),
            OwnedMimeInput::Provider { .. } => false,
        },
        Ok(_) => false,
    }
}

fn returned_same_async_bytes(
    result: Result<qubit_mime::AsyncMimeOperation, qubit_mime::MimeSubmitFailure>,
    original: &Arc<[u8]>,
) -> bool {
    match result {
        Err(failure) => match failure.input {
            OwnedMimeInput::Bytes { content, .. } => Arc::ptr_eq(original, &content),
            OwnedMimeInput::Provider { .. } => false,
        },
        Ok(_) => false,
    }
}
