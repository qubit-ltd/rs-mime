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
use std::thread;

use qubit_fs::AsyncFileSystem;
use qubit_fs::FsError;
use qubit_fs::FsResult;
use qubit_fs::Path;
use qubit_fs::error::FsErrorKind;
use qubit_fs::error::FsOperation;
use qubit_fs::metadata::FileSystemCapabilities;
use qubit_fs::metadata::FileSystemCapability;
use qubit_fs::metadata::FileSystemId;
use qubit_fs::metadata::FileSystemInfo;
use qubit_fs::metadata::FileSystemLimits;
use qubit_fs::metadata::OpenedFileInfo;
use qubit_fs::metadata::SymlinkPolicy;
use qubit_fs::path::PathConstraints;
use qubit_fs::path::PathSemantics;
use qubit_fs::spi::AsyncFileSystemSpi;
use qubit_fs::spi::OpenReaderRequest;
use qubit_fs::spi::OpenedAsyncReader;
use qubit_fs::spi::ProviderOperation;
use qubit_fs::spi::ProviderOperations;
use qubit_fs::spi::ProviderProperties;
use qubit_fs::spi::SpiFuture;
use qubit_fs::spi::StatRequest;
use qubit_fs::spi::StatResponse;
use qubit_io::AsyncInput;
use qubit_mime::FixedMimeExecutionPort;
use qubit_mime::MimeExecutionServices;
use qubit_mime::MimeOperationOptions;
use qubit_mime::OwnedMimeInput;
use qubit_mime::detector::MimeRuntime;
use qubit_operation::GroupOptions;
use qubit_operation::ManagerOptions;
use qubit_operation::OperationManager;
use qubit_operation::OperationOutcome;
use qubit_operation::RecoveryStore;
use qubit_operation::RecoveryStoreOptions;

fn managed_lifecycle(
    file_system: Arc<AsyncFileSystem>,
    recovery_store: &RecoveryStore,
    retry_queue: &mut Vec<OwnedMimeInput>,
) {
    let manager = OperationManager::new(ManagerOptions::default()).expect("operation manager");
    let group = manager
        .group(GroupOptions { capacity: 8 })
        .expect("MIME operation group");
    let services = MimeExecutionServices::new(
        manager,
        group,
        Arc::new(FixedMimeExecutionPort::new(1, 8)),
        1024 * 1024,
    )
    .expect("execution services");
    services.prepare().expect("prepare MIME services");

    let runtime = MimeRuntime::builtin()
        .expect("built-in repository")
        .with_execution_services(services.clone());
    let input = OwnedMimeInput::Provider {
        file_system,
        path: Path::parse("/upload/report.pdf").expect("provider path"),
    };
    let mut operation = match runtime.try_submit_detection(input, MimeOperationOptions::default()) {
        Ok(operation) => operation,
        Err(failure) => {
            eprintln!("submission rejected: {:?}", failure.reason);
            retry_queue.push(failure.input);
            let shutdown = services.shutdown();
            let settled =
                block_on(shutdown.wait_services_settled()).expect("settle service operations");
            assert!(settled.operations_settled);
            let stopped = shutdown.stop_services().expect("stop MIME services");
            assert!(stopped.services_stopped);
            let report = block_on(shutdown.wait_group_shutdown()).expect("drain MIME group");
            assert_eq!(report.snapshot.active_operations, 0);
            return;
        }
    };

    let outcome = block_on(operation.wait_outcome()).expect("observe MIME outcome");
    if let OperationOutcome::Succeeded(report) = outcome {
        println!(
            "{}: {:?} ({:?}, {} bytes read)",
            report.backend_id, report.mime_type, report.inspected_extent, report.budget.bytes_read,
        );
    }

    let settlement = block_on(operation.wait_settlement());
    assert!(
        settlement.all_released(),
        "operation resources should be settled"
    );
    drop(operation);

    let shutdown = services.shutdown();
    let settled = block_on(shutdown.wait_services_settled()).expect("settle tracked operations");
    assert!(settled.operations_settled);
    for recovery in shutdown.recovery_handles_snapshot() {
        recovery
            .transfer_to(recovery_store)
            .expect("transfer unresolved cleanup to the application recovery store");
    }
    let stopped = shutdown
        .stop_services()
        .expect("stop MIME services from this sync context");
    assert!(stopped.services_stopped && stopped.unresolved_recoveries == 0);
    let report = block_on(shutdown.wait_group_shutdown()).expect("wait for MIME group Drain");
    assert_eq!(report.snapshot.active_operations, 0);
}

struct MemoryProvider {
    id: FileSystemId,
    properties: ProviderProperties,
    content: Arc<[u8]>,
}

impl MemoryProvider {
    fn sample_pdf() -> Self {
        let id = FileSystemId::new("mime-doc-memory").expect("valid filesystem id");
        let properties = ProviderProperties::new(
            FileSystemInfo::new(id.clone(), "mime-doc-memory", PathSemantics::Hierarchical),
            ProviderOperations::new().with(ProviderOperation::OpenReader),
            FileSystemCapabilities::new().with_guaranteed(FileSystemCapability::Read),
            FileSystemLimits::unknown(),
            PathConstraints::absolute(),
            SymlinkPolicy::Reject,
        )
        .expect("valid sample provider properties");
        Self {
            id,
            properties,
            content: Arc::from(&b"%PDF-1.7\n"[..]),
        }
    }
}

impl AsyncFileSystemSpi for MemoryProvider {
    fn properties(&self) -> ProviderProperties {
        self.properties.clone()
    }

    fn stat<'a>(&'a self, request: StatRequest<'a>) -> SpiFuture<'a, FsResult<StatResponse>> {
        let path = request.path().clone();
        Box::pin(async move {
            Err(FsError::new(
                FsErrorKind::UnsupportedOperation,
                FsOperation::Stat,
                "the lifecycle example only supports opening its sample object",
            )
            .with_path(path))
        })
    }

    fn open_reader<'a>(
        &'a self,
        request: OpenReaderRequest<'a>,
    ) -> SpiFuture<'a, FsResult<OpenedAsyncReader>> {
        let path = request.path().clone();
        let info = OpenedFileInfo::new(self.id.clone(), path);
        let input = MemoryInput {
            content: Arc::clone(&self.content),
            position: 0,
        };
        Box::pin(async move { Ok(OpenedAsyncReader::new(info, Box::new(input))) })
    }
}

struct MemoryInput {
    content: Arc<[u8]>,
    position: usize,
}

impl AsyncInput for MemoryInput {
    type Item = u8;

    unsafe fn poll_read_unchecked(
        self: Pin<&mut Self>,
        _context: &mut Context<'_>,
        output: &mut [u8],
        index: usize,
        count: usize,
    ) -> Poll<std::io::Result<usize>> {
        let this = self.get_mut();
        let read = count.min(this.content.len().saturating_sub(this.position));
        output[index..index + read]
            .copy_from_slice(&this.content[this.position..this.position + read]);
        this.position += read;
        Poll::Ready(Ok(read))
    }
}

struct ThreadWake(thread::Thread);

impl Wake for ThreadWake {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}

fn block_on<F: Future>(future: F) -> F::Output {
    let waker = Waker::from(Arc::new(ThreadWake(thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(output) => return output,
            Poll::Pending => thread::park(),
        }
    }
}

pub fn run_example() {
    let file_system =
        Arc::new(AsyncFileSystem::from_spi(MemoryProvider::sample_pdf()).expect("memory provider"));
    let recovery_store =
        RecoveryStore::new(RecoveryStoreOptions::default()).expect("recovery store");
    let mut retry_queue = Vec::new();
    managed_lifecycle(file_system, &recovery_store, &mut retry_queue);
    assert!(
        retry_queue.is_empty(),
        "the sample submission should be accepted"
    );
    if recovery_store.snapshot().groups == 0 {
        recovery_store
            .request_shutdown()
            .wait()
            .expect("stop the empty recovery store");
    } else {
        eprintln!("the application must keep its recovery store alive for unresolved cleanup");
    }
}
