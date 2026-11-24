// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

//! Focused checks for staging bounds and fixed-worker admission.

use std::future::Future;
use std::io::Write;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Barrier;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::task::Context;
use std::task::Poll;
use std::task::Wake;
use std::task::Waker;

use qubit_fs::AsyncFileSystem;
use qubit_fs::FsResult;
use qubit_fs::Path as FsPath;
use qubit_fs::metadata::FileKind;
use qubit_fs::metadata::FileMetadata;
use qubit_fs::metadata::FileSystemCapabilities;
use qubit_fs::metadata::FileSystemId;
use qubit_fs::metadata::FileSystemInfo;
use qubit_fs::metadata::FileSystemLimits;
use qubit_fs::metadata::OpenedFileInfo;
use qubit_fs::metadata::SymlinkPolicy;
use qubit_fs::path::PathConstraints;
use qubit_fs::path::PathSemantics;
use qubit_fs::spi::AsyncFileSystemSpi;
use qubit_fs::spi::AsyncRandomAccessReadSession;
use qubit_fs::spi::OpenReaderRequest;
use qubit_fs::spi::OpenedAsyncRandomAccessReader;
use qubit_fs::spi::OpenedAsyncReader;
use qubit_fs::spi::ProviderOperation;
use qubit_fs::spi::ProviderOperations;
use qubit_fs::spi::ProviderProperties;
use qubit_fs::spi::SpiFuture;
use qubit_fs::spi::StatRequest;
use qubit_fs::spi::StatResponse;
use qubit_io::AsyncInput;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::options::LocalTempFileOptions;
use qubit_local_files::policy::LocalPrivacyRequirement;

use crate::InputExtent;
use crate::MimeError;
use crate::OwnedMimeInput;
use crate::command_execution::AsyncMimeCommandExecutor;
use crate::command_execution::FixedMimeExecutionPort;
use crate::command_execution::ManagedMimeCommand;
use crate::command_execution::MimeCommandOutput;
use crate::command_execution::MimeExecutionWorker;
use crate::detector::MimeExecutionPort;
use crate::detector::MimeExecutionServices;
use crate::detector::metered_read_session::MeteredReadSession;
use crate::managed_temp_staging::ManagedStagingLimits;
use crate::managed_temp_staging::StagedMimeInput;
use crate::managed_temp_staging::commit_staged;
use crate::managed_temp_staging::create_empty_staged_with;
use crate::managed_temp_staging::create_empty_staged_with_hook;
use crate::managed_temp_staging::recovery_cleanup_test_guard;
use crate::managed_temp_staging::sentinel_read_limit;
use crate::managed_temp_staging::stage_owned_in;
use crate::managed_temp_staging::within_staging_limit;

/// Unparks the polling thread when a test future schedules its waker.
struct ThreadWake(std::thread::Thread);
impl Wake for ThreadWake {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}
/// Runs a future on the current test thread, parking it between wakeups.
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

fn controlled_identity_hook() -> (
    crate::managed_temp_staging::PublishHook,
    std::sync::mpsc::Receiver<crate::managed_temp_staging::PublishEvent>,
    std::sync::mpsc::Sender<()>,
    std::sync::mpsc::Sender<()>,
) {
    use crate::managed_temp_staging::PublishEvent;

    let (event_tx, event_rx) = std::sync::mpsc::channel();
    let (pending_release_tx, pending_release_rx) = std::sync::mpsc::channel();
    let (publish_release_tx, publish_release_rx) = std::sync::mpsc::channel();
    let pending_release_rx = std::sync::Mutex::new(pending_release_rx);
    let publish_release_rx = std::sync::Mutex::new(publish_release_rx);
    let hook = Arc::new(move |event| {
        let release = match event {
            PublishEvent::CleanupPending => &pending_release_rx,
            PublishEvent::Publishing(_) => &publish_release_rx,
        };
        if event_tx.send(event).is_ok() {
            let _ = release
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .recv_timeout(std::time::Duration::from_secs(5));
        }
    });
    (hook, event_rx, pending_release_tx, publish_release_tx)
}

fn wait_for_publication_and_pending_cleanup(
    event_rx: &std::sync::mpsc::Receiver<crate::managed_temp_staging::PublishEvent>,
) -> qubit_operation::ResourceId {
    use crate::managed_temp_staging::PublishEvent;

    let mut published_id = None;
    let mut cleanup_pending = false;
    while published_id.is_none() || !cleanup_pending {
        match event_rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap() {
            PublishEvent::Publishing(id) => published_id = Some(id),
            PublishEvent::CleanupPending => cleanup_pending = true,
        }
    }
    published_id.expect("identity publication event should be observed")
}

#[derive(Debug)]
struct RejectCleanupAfterFirstDispatch {
    inner: Arc<FixedMimeExecutionPort>,
    dispatches: AtomicUsize,
}

impl MimeExecutionPort for RejectCleanupAfterFirstDispatch {
    fn prepare(&self) -> Result<(), MimeError> {
        self.inner.prepare()
    }

    fn try_dispatch(&self, job: Box<dyn FnOnce() + Send + 'static>) -> Result<(), Box<dyn FnOnce() + Send + 'static>> {
        if self.dispatches.fetch_add(1, Ordering::AcqRel) == 0 {
            self.inner.try_dispatch(job)
        } else {
            Err(job)
        }
    }

    fn shutdown(&self) -> Result<(), MimeError> {
        self.inner.shutdown()
    }
}

#[test]
fn test_managed_staging_limit_boundaries() {
    let limit = 8;
    assert!(within_staging_limit(limit - 1, limit));
    assert!(within_staging_limit(limit, limit));
    assert!(!within_staging_limit(limit + 1, limit));
}

#[test]
fn test_managed_staging_zero_limit() {
    assert!(within_staging_limit(0, 0));
    assert!(!within_staging_limit(1, 0));
}

#[test]
fn test_managed_staging_checked_sentinel_arithmetic() {
    assert_eq!(sentinel_read_limit(8), Some(9));
    assert_eq!(sentinel_read_limit(u64::MAX), None);
}

/// In-memory provider used to count opens and control asynchronous read
/// behavior.
struct BytesAsyncFs {
    properties: ProviderProperties,
    content: Vec<u8>,
    opens: Arc<AtomicUsize>,
    reads: Arc<AtomicUsize>,
    pending_read: Option<Arc<AtomicBool>>,
    first_read_fault: Option<ReadFault>,
}

#[derive(Clone, Copy)]
enum ReadFault {
    Error,
    OverreportedCount,
}

impl BytesAsyncFs {
    /// Builds a provider that returns the supplied bytes in short chunks.
    fn filesystem(content: &[u8]) -> (Arc<AsyncFileSystem>, Arc<AtomicUsize>, Arc<AtomicUsize>) {
        Self::filesystem_with_options(content, None, None)
    }

    /// Builds a provider that either returns short chunks or remains pending on
    /// read.
    fn filesystem_with_pending(
        content: &[u8],
        pending_read: Option<Arc<AtomicBool>>,
    ) -> (Arc<AsyncFileSystem>, Arc<AtomicUsize>, Arc<AtomicUsize>) {
        Self::filesystem_with_options(content, pending_read, None)
    }

    /// Builds a provider that reports either a read error or an invalid count.
    fn filesystem_with_fault(
        content: &[u8],
        fault: ReadFault,
    ) -> (Arc<AsyncFileSystem>, Arc<AtomicUsize>, Arc<AtomicUsize>) {
        Self::filesystem_with_options(content, None, Some(fault))
    }

    fn filesystem_with_options(
        content: &[u8],
        pending_read: Option<Arc<AtomicBool>>,
        first_read_fault: Option<ReadFault>,
    ) -> (Arc<AsyncFileSystem>, Arc<AtomicUsize>, Arc<AtomicUsize>) {
        let properties = ProviderProperties::new(
            FileSystemInfo::new(
                FileSystemId::new("managed-staging-test").unwrap(),
                "managed-staging-test",
                PathSemantics::Hierarchical,
            ),
            ProviderOperations::new()
                .with(ProviderOperation::OpenReader)
                .with(ProviderOperation::OpenRandomAccessReader),
            FileSystemCapabilities::new().with_guaranteed(qubit_fs::metadata::FileSystemCapability::Read),
            FileSystemLimits::unknown(),
            PathConstraints::absolute(),
            SymlinkPolicy::Reject,
        )
        .unwrap();
        let opens = Arc::new(AtomicUsize::new(0));
        let reads = Arc::new(AtomicUsize::new(0));
        let fs = AsyncFileSystem::from_spi(Self {
            properties,
            content: content.to_vec(),
            opens: Arc::clone(&opens),
            reads: Arc::clone(&reads),
            pending_read,
            first_read_fault,
        })
        .unwrap();
        (Arc::new(fs), opens, reads)
    }
}

impl AsyncFileSystemSpi for BytesAsyncFs {
    fn properties(&self) -> ProviderProperties {
        self.properties.clone()
    }
    fn stat<'a>(&'a self, _request: StatRequest<'a>) -> SpiFuture<'a, FsResult<StatResponse>> {
        Box::pin(async { panic!("staging must not stat provider input") })
    }
    fn open_reader<'a>(&'a self, request: OpenReaderRequest<'a>) -> SpiFuture<'a, FsResult<OpenedAsyncReader>> {
        self.opens.fetch_add(1, Ordering::SeqCst);
        let info = OpenedFileInfo::new(
            FileSystemId::new("managed-staging-test").unwrap(),
            request.path().clone(),
        );
        if let Some(polled) = self.pending_read.as_ref().map(Arc::clone) {
            Box::pin(async move { Ok(OpenedAsyncReader::new(info, Box::new(PendingAsyncInput { polled }))) })
        } else {
            let bytes = self.content.clone();
            let reads = Arc::clone(&self.reads);
            let first_read_fault = self.first_read_fault;
            Box::pin(async move {
                Ok(OpenedAsyncReader::new(
                    info,
                    Box::new(ShortAsyncInput {
                        bytes,
                        at: 0,
                        reads,
                        first_read_fault,
                    }),
                ))
            })
        }
    }

    fn open_random_access_reader<'a>(
        &'a self,
        request: OpenReaderRequest<'a>,
    ) -> SpiFuture<'a, FsResult<OpenedAsyncRandomAccessReader>> {
        self.opens.fetch_add(1, Ordering::SeqCst);
        let bytes = self.content.clone();
        let reads = Arc::clone(&self.reads);
        let first_read_fault = self.first_read_fault;
        let info = OpenedFileInfo::new(
            FileSystemId::new("managed-staging-test").unwrap(),
            request.path().clone(),
        )
        .with_metadata(FileMetadata::new(FileKind::File).with_len(Some(bytes.len() as u64)));
        Box::pin(async move {
            Ok(OpenedAsyncRandomAccessReader::new(
                info,
                Box::new(BytesRandomAccessSession {
                    bytes,
                    reads,
                    first_read_fault,
                }),
            ))
        })
    }
}

/// Reader that stays pending and records when the provider polls it.
struct PendingAsyncInput {
    polled: Arc<AtomicBool>,
}
impl AsyncInput for PendingAsyncInput {
    type Item = u8;
    unsafe fn poll_read_unchecked(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        _output: &mut [u8],
        _index: usize,
        _count: usize,
    ) -> Poll<std::io::Result<usize>> {
        self.polled.store(true, Ordering::SeqCst);
        Poll::Pending
    }
}

/// Reader that returns at most two bytes per poll and counts bytes read.
struct ShortAsyncInput {
    bytes: Vec<u8>,
    at: usize,
    reads: Arc<AtomicUsize>,
    first_read_fault: Option<ReadFault>,
}
impl AsyncInput for ShortAsyncInput {
    type Item = u8;
    unsafe fn poll_read_unchecked(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        output: &mut [u8],
        index: usize,
        count: usize,
    ) -> Poll<std::io::Result<usize>> {
        match self.first_read_fault.take() {
            Some(ReadFault::Error) => {
                return Poll::Ready(Err(std::io::Error::other("injected provider read error")));
            }
            Some(ReadFault::OverreportedCount) => return Poll::Ready(Ok(count + 1)),
            None => {}
        }
        let available = self.bytes.len().saturating_sub(self.at);
        let read = count.min(available).min(2);
        output[index..index + read].copy_from_slice(&self.bytes[self.at..self.at + read]);
        self.at += read;
        self.reads.fetch_add(read, Ordering::SeqCst);
        Poll::Ready(Ok(read))
    }
}

struct BytesRandomAccessSession {
    bytes: Vec<u8>,
    reads: Arc<AtomicUsize>,
    first_read_fault: Option<ReadFault>,
}

impl AsyncRandomAccessReadSession for BytesRandomAccessSession {
    fn read_at_async<'a>(self: Pin<&'a mut Self>, offset: u64, output: &'a mut [u8]) -> SpiFuture<'a, FsResult<usize>> {
        Box::pin(async move {
            let this = self.get_mut();
            match this.first_read_fault.take() {
                Some(ReadFault::Error) => {
                    return Err(qubit_fs::FsError::new(
                        qubit_fs::error::FsErrorKind::Io,
                        qubit_fs::error::FsOperation::Read,
                        "injected random-access read failure",
                    ));
                }
                Some(ReadFault::OverreportedCount) => return Ok(output.len() + 1),
                None => {}
            }
            let offset = usize::try_from(offset).unwrap_or(usize::MAX);
            let count = output.len().min(this.bytes.len().saturating_sub(offset));
            output[..count].copy_from_slice(&this.bytes[offset..offset + count]);
            this.reads.fetch_add(count, Ordering::SeqCst);
            Ok(count)
        })
    }
}

#[test]
fn test_managed_staging_provider_limit_boundaries_and_short_reads() {
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let group = manager.group(qubit_operation::GroupOptions { capacity: 8 }).unwrap();
    let port = Arc::new(FixedMimeExecutionPort::new(1, 8));
    let services = Arc::new(MimeExecutionServices::new(manager, group.clone(), port.clone(), 64).unwrap());
    services.prepare().unwrap();

    for (content, should_succeed, actual_read) in [
        (b"ab".as_slice(), true, 2),
        (b"abc", true, 3),
        (b"abcd", false, 4), // includes the one-byte sentinel
        (b"", true, 0),
    ] {
        let (filesystem, opens, reads) = BytesAsyncFs::filesystem(content);
        let read_tree = services.new_read_tree(16);
        let body_read_tree = Arc::clone(&read_tree);
        let body_services = Arc::clone(&services);
        let input = OwnedMimeInput::Provider {
            file_system: filesystem,
            path: FsPath::parse("/input").unwrap(),
        };
        let limits = ManagedStagingLimits {
            max_bytes: 3,
            budget: services.staging_budget(),
            worker: MimeExecutionWorker::new(port.clone()),
        };
        let mut handle = block_on(group.try_submit_async(Default::default(), move |scope| async move {
            body_services.bind_read_tree(scope.id(), &body_read_tree);
            let read_tree = body_services.read_tree_for_operation(scope.id())?;
            stage_owned_in(&scope, input, limits, read_tree).await
        }))
        .unwrap();
        match block_on(handle.wait_outcome()).unwrap() {
            qubit_operation::OperationOutcome::Succeeded(resource) => {
                assert!(should_succeed, "oversized input must fail");
                assert_eq!(content, std::fs::read(resource.path().unwrap()).unwrap().as_slice());
                drop(resource);
            }
            qubit_operation::OperationOutcome::Failed(_) => {
                assert!(!should_succeed, "bounded input should stage")
            }
            _ => panic!("unexpected provider staging outcome"),
        }
        let report = block_on(handle.wait_settlement());
        assert!(report.all_released());
        assert_eq!(1, opens.load(Ordering::SeqCst), "provider object opens exactly once");
        assert_eq!(actual_read, reads.load(Ordering::SeqCst));
        assert_eq!(16_u64 - actual_read as u64, read_tree.remaining());
    }
    let shutdown = services.shutdown();
    let _ = block_on(shutdown.wait_services_settled()).unwrap();
    let _ = shutdown.stop_services().unwrap();
}

#[test]
fn test_managed_staging_provider_read_tree_budget_stops_before_sentinel_overrun() {
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let group = manager.group(qubit_operation::GroupOptions { capacity: 4 }).unwrap();
    let port = Arc::new(FixedMimeExecutionPort::new(1, 4));
    let services = MimeExecutionServices::new(manager, group.clone(), port.clone(), 16).unwrap();
    services.prepare().unwrap();
    let (filesystem, _, reads) = BytesAsyncFs::filesystem(b"abcd");
    let input = OwnedMimeInput::Provider {
        file_system: filesystem,
        path: FsPath::parse("/input").unwrap(),
    };
    let limits = ManagedStagingLimits {
        max_bytes: 3,
        budget: services.staging_budget(),
        worker: MimeExecutionWorker::new(port),
    };
    let read_tree = services.new_read_tree(3);
    let body_read_tree = Arc::clone(&read_tree);
    let mut handle = block_on(group.try_submit_async(Default::default(), move |scope| async move {
        stage_owned_in(&scope, input, limits, body_read_tree).await
    }))
    .unwrap();
    assert!(matches!(
        block_on(handle.wait_outcome()).unwrap(),
        qubit_operation::OperationOutcome::Failed(_)
    ));
    assert_eq!(3, reads.load(Ordering::SeqCst));
    assert_eq!(0, read_tree.remaining());
    assert!(block_on(handle.wait_settlement()).all_released());
    let shutdown = services.shutdown();
    let _ = block_on(shutdown.wait_services_settled()).unwrap();
    let _ = shutdown.stop_services().unwrap();
}

#[test]
fn test_managed_staging_provider_budget_failure_cleans_partial_owner() {
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let group = manager.group(qubit_operation::GroupOptions { capacity: 4 }).unwrap();
    let port = Arc::new(FixedMimeExecutionPort::new(1, 4));
    // Leave room for one full staging chunk in the fixed service budget.
    let services = MimeExecutionServices::new(manager, group.clone(), port.clone(), 16).unwrap();
    services.prepare().unwrap();
    let occupied = services
        .staging_budget()
        .try_reserve(256 * 1024 * 1024 - 8 * 1024)
        .unwrap();
    let content = vec![b'x'; 8 * 1024 + 1];
    let (filesystem, opens, _) = BytesAsyncFs::filesystem(&content);
    let input = OwnedMimeInput::Provider {
        file_system: filesystem,
        path: FsPath::parse("/input").unwrap(),
    };
    let limits = ManagedStagingLimits {
        max_bytes: content.len() as u64,
        budget: services.staging_budget(),
        worker: MimeExecutionWorker::new(port),
    };
    let read_tree = services.new_read_tree(content.len() as u64);
    let body_read_tree = Arc::clone(&read_tree);
    let mut handle = block_on(group.try_submit_async(Default::default(), move |scope| async move {
        stage_owned_in(&scope, input, limits, body_read_tree).await
    }))
    .unwrap();
    assert!(matches!(
        block_on(handle.wait_outcome()).unwrap(),
        qubit_operation::OperationOutcome::Failed(_)
    ));
    assert_eq!(1, opens.load(Ordering::SeqCst));
    assert_eq!(0, read_tree.remaining(), "staging read bytes count cumulatively");
    let report = block_on(handle.wait_settlement());
    assert!(report.all_released(), "partial staged owner must be cleaned");
    assert_eq!(
        256 * 1024 * 1024 - 8 * 1024,
        services.staging_budget().used_bytes(),
        "only the test's held budget reservation remains after cleanup"
    );
    drop(occupied);
    assert!(services.staging_budget().try_reserve(256 * 1024 * 1024).is_some());
    let shutdown = services.shutdown();
    let _ = block_on(shutdown.wait_services_settled()).unwrap();
    let _ = shutdown.stop_services().unwrap();
}

#[test]
fn test_managed_staging_provider_read_faults_roll_back_and_clean_resources() {
    for fault in [ReadFault::Error, ReadFault::OverreportedCount] {
        let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
        let group = manager.group(qubit_operation::GroupOptions { capacity: 2 }).unwrap();
        let port = Arc::new(FixedMimeExecutionPort::new(1, 2));
        let services = MimeExecutionServices::new(manager, group.clone(), port.clone(), 8).unwrap();
        services.prepare().unwrap();
        let (filesystem, opens, reads) = BytesAsyncFs::filesystem_with_fault(b"payload", fault);
        let read_tree = services.new_read_tree(8);
        let input = OwnedMimeInput::Provider {
            file_system: filesystem,
            path: FsPath::parse("/input").unwrap(),
        };
        let limits = ManagedStagingLimits {
            max_bytes: 8,
            budget: services.staging_budget(),
            worker: MimeExecutionWorker::new(port.clone()),
        };
        let body_read_tree = Arc::clone(&read_tree);
        let mut handle = block_on(group.try_submit_async(Default::default(), move |scope| async move {
            stage_owned_in(&scope, input, limits, body_read_tree).await
        }))
        .unwrap();

        match block_on(handle.wait_outcome()).unwrap() {
            qubit_operation::OperationOutcome::Failed(MimeError::Io(error)) => {
                let expected_kind = match fault {
                    ReadFault::Error => std::io::ErrorKind::Other,
                    ReadFault::OverreportedCount => std::io::ErrorKind::InvalidData,
                };
                assert_eq!(expected_kind, error.kind());
            }
            _ => panic!("provider read fault must be reported as I/O failure"),
        }
        assert_eq!(1, opens.load(Ordering::SeqCst));
        assert_eq!(0, reads.load(Ordering::SeqCst));
        assert_eq!(0, read_tree.consumed(), "failed reads transfer no bytes");
        assert_eq!(8, read_tree.remaining(), "failed reads release their reservation");
        assert!(block_on(handle.wait_settlement()).all_released());
        assert_eq!(0, services.staging_budget().used_bytes(), "failed staging is cleaned");
        let shutdown = services.shutdown();
        let _ = block_on(shutdown.wait_services_settled()).unwrap();
        let _ = shutdown.stop_services().unwrap();
    }
}

#[test]
fn test_managed_staging_provider_work_buffer_budget_failure_cleans_empty_owner() {
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let group = manager.group(qubit_operation::GroupOptions { capacity: 2 }).unwrap();
    let port = Arc::new(FixedMimeExecutionPort::new(1, 2));
    let services = MimeExecutionServices::new(manager, group.clone(), port.clone(), 8).unwrap();
    services.prepare().unwrap();
    let budget = services.staging_budget();
    let occupied = budget.try_reserve_owned(16 * 1024 * 1024).unwrap();
    let (filesystem, opens, reads) = BytesAsyncFs::filesystem(b"payload");
    let read_tree = services.new_read_tree(8);
    let input = OwnedMimeInput::Provider {
        file_system: filesystem,
        path: FsPath::parse("/input").unwrap(),
    };
    let limits = ManagedStagingLimits {
        max_bytes: 8,
        budget: budget.clone(),
        worker: MimeExecutionWorker::new(port.clone()),
    };
    let body_read_tree = Arc::clone(&read_tree);
    let mut handle = block_on(group.try_submit_async(Default::default(), move |scope| async move {
        stage_owned_in(&scope, input, limits, body_read_tree).await
    }))
    .unwrap();

    assert!(matches!(
        block_on(handle.wait_outcome()).unwrap(),
        qubit_operation::OperationOutcome::Failed(MimeError::BudgetExceeded {
            kind: "owned",
            requested: 8_192,
            ..
        })
    ));
    assert_eq!(1, opens.load(Ordering::SeqCst));
    assert_eq!(
        0,
        reads.load(Ordering::SeqCst),
        "the provider is not read without its work buffer"
    );
    assert_eq!(8, read_tree.remaining());
    assert!(block_on(handle.wait_settlement()).all_released());
    assert_eq!(0, budget.used_bytes(), "the empty staged owner is cleaned");
    drop(occupied);
    assert!(budget.try_reserve_owned(16 * 1024 * 1024).is_some());
    let shutdown = services.shutdown();
    let _ = block_on(shutdown.wait_services_settled()).unwrap();
    let _ = shutdown.stop_services().unwrap();
}

#[test]
fn test_managed_staging_rejects_oversized_owned_input_before_file_creation() {
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let group = manager.group(qubit_operation::GroupOptions { capacity: 1 }).unwrap();
    let port = Arc::new(FixedMimeExecutionPort::new(1, 1));
    let services = MimeExecutionServices::new(manager, group.clone(), port.clone(), 8).unwrap();
    services.prepare().unwrap();
    let input = OwnedMimeInput::bytes(Arc::from(b"too large".as_slice()), InputExtent::Complete);
    let limits = ManagedStagingLimits {
        max_bytes: 4,
        budget: services.staging_budget(),
        worker: MimeExecutionWorker::new(port.clone()),
    };
    let read_tree = services.new_read_tree(8);
    let body_read_tree = Arc::clone(&read_tree);
    let mut handle = block_on(group.try_submit_async(Default::default(), move |scope| async move {
        stage_owned_in(&scope, input, limits, body_read_tree).await
    }))
    .unwrap();

    assert!(matches!(
        block_on(handle.wait_outcome()).unwrap(),
        qubit_operation::OperationOutcome::Failed(MimeError::BudgetExceeded {
            kind: "staging",
            requested: 9,
            limit: 4,
        })
    ));
    assert_eq!(0, services.staging_budget().used_bytes());
    assert_eq!(8, read_tree.remaining());
    assert!(block_on(handle.wait_settlement()).all_released());
    let shutdown = services.shutdown();
    let _ = block_on(shutdown.wait_services_settled()).unwrap();
    let _ = shutdown.stop_services().unwrap();
}

#[test]
fn test_metered_random_access_reads_share_budget_and_reject_sequential_offsets() {
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let group = manager.group(qubit_operation::GroupOptions { capacity: 1 }).unwrap();
    let port = Arc::new(FixedMimeExecutionPort::new(1, 1));
    let services = Arc::new(MimeExecutionServices::new(manager, group, port.clone(), 8).unwrap());
    services.prepare().unwrap();
    let (filesystem, opens, reads) = BytesAsyncFs::filesystem(b"012345");
    let path = FsPath::parse("/input").unwrap();
    let read_tree = services.new_read_tree(5);
    let mut session = block_on(MeteredReadSession::open_random_access(
        &filesystem,
        &path,
        Arc::clone(&services),
        Arc::clone(&read_tree),
        qubit_operation::OperationContext::new(Default::default()).unwrap(),
        8,
        4,
    ))
    .unwrap();
    let mut output = [0xCC; 4];
    assert_eq!(4, block_on(session.read_at(1, &mut output)).unwrap());
    assert_eq!(*b"1234", output);
    output.fill(0xCC);
    assert_eq!(1, block_on(session.read_at(2, &mut output)).unwrap());
    assert_eq!([b'2', 0xCC, 0xCC, 0xCC], output);
    assert_eq!(5, read_tree.consumed());
    assert!(matches!(
        block_on(session.read_at(0, &mut output)),
        Err(MimeError::BudgetExceeded { kind: "read", .. })
    ));
    assert_eq!(5, reads.load(Ordering::SeqCst));
    assert_eq!(1, opens.load(Ordering::SeqCst));

    let (failing_filesystem, failing_opens, failing_reads) =
        BytesAsyncFs::filesystem_with_fault(b"012345", ReadFault::Error);
    let failing_tree = services.new_read_tree(4);
    let mut failing_session = block_on(MeteredReadSession::open_random_access(
        &failing_filesystem,
        &path,
        Arc::clone(&services),
        Arc::clone(&failing_tree),
        qubit_operation::OperationContext::new(Default::default()).unwrap(),
        4,
        4,
    ))
    .unwrap();
    output.fill(0xCC);
    assert!(matches!(
        block_on(failing_session.read_at(0, &mut output)),
        Err(MimeError::FileSystem(_))
    ));
    assert_eq!(
        [0xCC; 4], output,
        "failed random reads keep private bytes out of output"
    );
    assert_eq!(4, failing_tree.remaining(), "failed random reads release the permit");
    assert_eq!(1, failing_opens.load(Ordering::SeqCst));
    assert_eq!(0, failing_reads.load(Ordering::SeqCst));

    let sequential_tree = services.new_read_tree(4);
    let mut sequential = block_on(MeteredReadSession::open_sequential(
        &filesystem,
        &path,
        Arc::clone(&services),
        Arc::clone(&sequential_tree),
        qubit_operation::OperationContext::new(Default::default()).unwrap(),
        4,
        4,
    ))
    .unwrap();
    assert!(matches!(
        block_on(sequential.read_at(0, &mut output)),
        Err(MimeError::InvalidClassifierInput { .. })
    ));
    assert_eq!(0, sequential_tree.consumed());
    let shutdown = services.shutdown();
    let _ = block_on(shutdown.wait_services_settled()).unwrap();
    let _ = shutdown.stop_services().unwrap();
}

#[test]
fn test_metered_random_access_stop_after_first_read_blocks_next_provider_call() {
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let group = manager.group(qubit_operation::GroupOptions { capacity: 1 }).unwrap();
    let services =
        Arc::new(MimeExecutionServices::new(manager, group, Arc::new(FixedMimeExecutionPort::new(1, 1)), 8).unwrap());
    services.prepare().unwrap();
    let (filesystem, opens, reads) = BytesAsyncFs::filesystem(b"012345");
    let path = FsPath::parse("/input").unwrap();
    let tree = services.new_read_tree(8);
    let context = qubit_operation::OperationContext::new(Default::default()).unwrap();
    let mut session = block_on(MeteredReadSession::open_random_access(
        &filesystem,
        &path,
        Arc::clone(&services),
        Arc::clone(&tree),
        context.clone(),
        8,
        4,
    ))
    .unwrap();
    let mut output = [0; 4];
    let delivered = block_on(session.read_at(0, &mut output)).unwrap();
    let calls_after_first_read = reads.load(Ordering::SeqCst);
    assert!(calls_after_first_read > 0);
    assert!(context.request_stop(qubit_operation::StopReason::UserCancelled));
    assert!(block_on(session.read_at(1, &mut output)).is_err());
    assert_eq!(
        calls_after_first_read,
        reads.load(Ordering::SeqCst),
        "stopped session must not call the provider again"
    );
    assert_eq!(1, opens.load(Ordering::SeqCst));
    assert_eq!(
        delivered as u64,
        tree.consumed(),
        "successful first read remains charged"
    );
    let shutdown = services.shutdown();
    let _ = block_on(shutdown.wait_services_settled()).unwrap();
    let _ = shutdown.stop_services().unwrap();
}

#[test]
fn test_metered_sequential_stop_after_first_read_blocks_next_provider_call() {
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let group = manager.group(qubit_operation::GroupOptions { capacity: 1 }).unwrap();
    let services =
        Arc::new(MimeExecutionServices::new(manager, group, Arc::new(FixedMimeExecutionPort::new(1, 1)), 8).unwrap());
    services.prepare().unwrap();
    let (filesystem, opens, reads) = BytesAsyncFs::filesystem(b"012345");
    let path = FsPath::parse("/input").unwrap();
    let tree = services.new_read_tree(8);
    let context = qubit_operation::OperationContext::new(Default::default()).unwrap();
    let mut session = block_on(MeteredReadSession::open_sequential(
        &filesystem,
        &path,
        Arc::clone(&services),
        Arc::clone(&tree),
        context.clone(),
        8,
        4,
    ))
    .unwrap();
    let mut output = [0; 4];
    let delivered = block_on(session.read(&mut output)).unwrap();
    let calls_after_first_read = reads.load(Ordering::SeqCst);
    assert!(calls_after_first_read > 0);
    assert!(context.request_stop(qubit_operation::StopReason::UserCancelled));
    assert!(block_on(session.read(&mut output)).is_err());
    assert_eq!(
        calls_after_first_read,
        reads.load(Ordering::SeqCst),
        "stopped session must not call the provider again"
    );
    assert_eq!(1, opens.load(Ordering::SeqCst));
    assert_eq!(
        delivered as u64,
        tree.consumed(),
        "successful first read remains charged"
    );
    let shutdown = services.shutdown();
    let _ = block_on(shutdown.wait_services_settled()).unwrap();
    let _ = shutdown.stop_services().unwrap();
}

struct ControlledReadClock {
    domain: qubit_operation::ClockDomainId,
    ticks: std::sync::atomic::AtomicU64,
    expire_after_calls: Option<usize>,
    calls: AtomicUsize,
}

impl qubit_operation::ClockSource for ControlledReadClock {
    fn now(&self) -> qubit_operation::MonotonicTime {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        let ticks = if self.expire_after_calls.is_some_and(|limit| call >= limit) {
            5
        } else {
            self.ticks.load(Ordering::SeqCst)
        };
        qubit_operation::MonotonicTime::new(self.domain, ticks)
    }

    fn register_deadline(
        &self,
        _at: qubit_operation::MonotonicTime,
        _wake: Waker,
    ) -> Box<dyn qubit_operation::TimerRegistration> {
        Box::new(NoopReadTimer)
    }
}

struct NoopReadTimer;

impl qubit_operation::TimerRegistration for NoopReadTimer {
    fn cancel(self: Box<Self>) {}
}

fn deadline_context(clock: Arc<ControlledReadClock>) -> qubit_operation::OperationContext {
    qubit_operation::OperationContext::new(qubit_operation::ContextOptions {
        clock: Some(clock.clone()),
        deadline: Some(qubit_operation::Deadline::at(qubit_operation::MonotonicTime::new(
            clock.domain,
            5,
        ))),
        ..Default::default()
    })
    .unwrap()
}

#[test]
fn test_metered_final_stop_check_prevents_io_after_expiry_during_preparation() {
    for random_access in [false, true] {
        let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
        let group = manager.group(qubit_operation::GroupOptions { capacity: 1 }).unwrap();
        let services = Arc::new(
            MimeExecutionServices::new(manager, group, Arc::new(FixedMimeExecutionPort::new(1, 1)), 8).unwrap(),
        );
        services.prepare().unwrap();
        let (filesystem, opens, reads) = BytesAsyncFs::filesystem(b"012345");
        let path = FsPath::parse("/input").unwrap();
        let tree = services.new_read_tree(8);
        let domain = qubit_operation::ClockDomainId::new_unique();
        let clock = Arc::new(ControlledReadClock {
            domain,
            ticks: std::sync::atomic::AtomicU64::new(0),
            expire_after_calls: Some(3),
            calls: AtomicUsize::new(0),
        });
        let context = deadline_context(clock);
        let mut session = if random_access {
            block_on(MeteredReadSession::open_random_access(
                &filesystem,
                &path,
                Arc::clone(&services),
                Arc::clone(&tree),
                context,
                8,
                4,
            ))
        } else {
            block_on(MeteredReadSession::open_sequential(
                &filesystem,
                &path,
                Arc::clone(&services),
                Arc::clone(&tree),
                context,
                8,
                4,
            ))
        }
        .unwrap();
        let mut output = [0; 4];
        let result = if random_access {
            block_on(session.read_at(0, &mut output))
        } else {
            block_on(session.read(&mut output))
        };
        assert!(
            result.is_err(),
            "expired operation must be rejected before Provider I/O"
        );
        assert_eq!(1, opens.load(Ordering::SeqCst));
        assert_eq!(
            0,
            reads.load(Ordering::SeqCst),
            "deadline reached during preparation must block the Provider call"
        );
        assert_eq!(8, tree.remaining(), "the unstarted read reservation is returned");
        let shutdown = services.shutdown();
        let _ = block_on(shutdown.wait_services_settled()).unwrap();
        let _ = shutdown.stop_services().unwrap();
    }
}

#[test]
fn test_metered_open_rejects_stopped_and_expired_context_before_provider_io() {
    for random_access in [false, true] {
        for expired in [false, true] {
            let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
            let group = manager.group(qubit_operation::GroupOptions { capacity: 1 }).unwrap();
            let services = Arc::new(
                MimeExecutionServices::new(manager, group, Arc::new(FixedMimeExecutionPort::new(1, 1)), 8).unwrap(),
            );
            services.prepare().unwrap();
            let (filesystem, opens, _) = BytesAsyncFs::filesystem(b"012345");
            let path = FsPath::parse("/input").unwrap();
            let tree = services.new_read_tree(8);
            let context = if expired {
                let domain = qubit_operation::ClockDomainId::new_unique();
                deadline_context(Arc::new(ControlledReadClock {
                    domain,
                    ticks: std::sync::atomic::AtomicU64::new(5),
                    expire_after_calls: None,
                    calls: AtomicUsize::new(0),
                }))
            } else {
                let context = qubit_operation::OperationContext::new(Default::default()).unwrap();
                assert!(context.request_stop(qubit_operation::StopReason::UserCancelled));
                context
            };
            let result = if random_access {
                block_on(MeteredReadSession::open_random_access(
                    &filesystem,
                    &path,
                    Arc::clone(&services),
                    Arc::clone(&tree),
                    context,
                    8,
                    4,
                ))
            } else {
                block_on(MeteredReadSession::open_sequential(
                    &filesystem,
                    &path,
                    Arc::clone(&services),
                    Arc::clone(&tree),
                    context,
                    8,
                    4,
                ))
            };
            assert!(result.is_err());
            assert_eq!(
                0,
                opens.load(Ordering::SeqCst),
                "stopped/expired open must not call Provider"
            );
            let shutdown = services.shutdown();
            let _ = block_on(shutdown.wait_services_settled()).unwrap();
            let _ = shutdown.stop_services().unwrap();
        }
    }
}

#[test]
fn test_managed_staging_cancel_during_provider_read_retains_cleanup_owner() {
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let group = manager.group(qubit_operation::GroupOptions { capacity: 4 }).unwrap();
    let port = Arc::new(FixedMimeExecutionPort::new(1, 4));
    let services = MimeExecutionServices::new(manager, group.clone(), port.clone(), 16).unwrap();
    services.prepare().unwrap();
    let read_polled = Arc::new(AtomicBool::new(false));
    let (filesystem, _, _) = BytesAsyncFs::filesystem_with_pending(b"unused", Some(Arc::clone(&read_polled)));
    let input = OwnedMimeInput::Provider {
        file_system: filesystem,
        path: FsPath::parse("/input").unwrap(),
    };
    let limits = ManagedStagingLimits {
        max_bytes: 8,
        budget: services.staging_budget(),
        worker: MimeExecutionWorker::new(port),
    };
    let read_tree = services.new_read_tree(8);
    let body_read_tree = Arc::clone(&read_tree);
    let mut handle = block_on(group.try_submit_async(Default::default(), move |scope| async move {
        let mut stage = Box::pin(stage_owned_in(&scope, input, limits, body_read_tree));
        std::future::poll_fn(|cx| {
            match stage.as_mut().poll(cx) {
                Poll::Ready(_) => panic!("provider read should remain pending"),
                Poll::Pending => {}
            }
            if read_polled.load(Ordering::SeqCst) {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        })
        .await;
        // Simulate cancellation of the caller while the provider reader
        // is pending, after the temp owner has already been registered.
        drop(stage);
        Ok::<(), crate::MimeError>(())
    }))
    .unwrap();
    assert!(matches!(
        block_on(handle.wait_outcome()).unwrap(),
        qubit_operation::OperationOutcome::Succeeded(())
    ));
    let report = block_on(handle.wait_settlement());
    assert!(
        report.all_released(),
        "the registered temp owner is cleaned after cancellation"
    );
    assert!(
        services.staging_budget().try_reserve(16).is_some(),
        "any permits remain attached to the retained owner until cleanup"
    );
    assert_eq!(8, read_tree.remaining(), "a Pending read transfers zero bytes");
    let shutdown = services.shutdown();
    let _ = block_on(shutdown.wait_services_settled()).unwrap();
    let _ = shutdown.stop_services().unwrap();
}

#[test]
fn test_managed_staging_cancel_before_create() {
    let worker = FixedMimeExecutionPort::new(1, 1);
    let called = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&called);
    let job: Box<dyn FnOnce() + Send> = Box::new(move || flag.store(true, Ordering::SeqCst));
    let returned = worker
        .try_dispatch(job)
        .expect_err("unprepared worker rejects without running");
    drop(returned);
    assert!(!called.load(Ordering::SeqCst));
}

#[test]
fn test_managed_staging_worker_debug_reports_lifecycle_state() {
    let worker = FixedMimeExecutionPort::new(2, 3);
    let configured = format!("{worker:?}");
    assert!(configured.contains("worker_count: 2"));
    assert!(configured.contains("prepared: false"));
    assert!(configured.contains("stopped: false"));

    worker.prepare().unwrap();
    let prepared = format!("{worker:?}");
    assert!(prepared.contains("prepared: true"));

    worker.shutdown().unwrap();
    let stopped = format!("{worker:?}");
    assert!(stopped.contains("stopped: true"));
}

#[test]
fn test_managed_staging_worker_queue_is_bounded_and_shutdown_joins() {
    let worker = FixedMimeExecutionPort::new(1, 1);
    worker.prepare().unwrap();
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let entered_job = Arc::clone(&entered);
    let release_job = Arc::clone(&release);
    assert!(
        worker
            .try_dispatch(Box::new(move || {
                entered_job.wait();
                release_job.wait();
            }))
            .is_ok()
    );
    entered.wait();
    assert!(worker.try_dispatch(Box::new(|| {})).is_ok());
    assert!(worker.try_dispatch(Box::new(|| {})).is_err());
    let release_thread = std::thread::spawn(move || release.wait());
    worker.shutdown().unwrap();
    release_thread.join().unwrap();
    assert!(
        worker.prepare().is_err(),
        "shutdown permanently closes worker preparation"
    );
    assert!(worker.try_dispatch(Box::new(|| {})).is_err());
}

#[test]
fn test_managed_staging_prepare_shutdown_race_is_terminal() {
    for _ in 0..64 {
        let worker = Arc::new(FixedMimeExecutionPort::new(1, 1));
        let gate = Arc::new(Barrier::new(3));
        let prepare_worker = Arc::clone(&worker);
        let prepare_gate = Arc::clone(&gate);
        let prepare = std::thread::spawn(move || {
            prepare_gate.wait();
            prepare_worker.prepare()
        });
        let shutdown_worker = Arc::clone(&worker);
        let shutdown_gate = Arc::clone(&gate);
        let shutdown = std::thread::spawn(move || {
            shutdown_gate.wait();
            shutdown_worker.shutdown()
        });
        gate.wait();
        let _ = prepare.join().unwrap();
        shutdown.join().unwrap().unwrap();
        assert!(worker.prepare().is_err());
        assert!(worker.try_dispatch(Box::new(|| {})).is_err());
    }
}

#[test]
fn test_managed_staging_owner_waits_for_scope_end_and_all_references() {
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let group = manager.group(qubit_operation::GroupOptions { capacity: 4 }).unwrap();
    let port = Arc::new(FixedMimeExecutionPort::new(1, 4));
    let services = MimeExecutionServices::new(manager, group.clone(), port.clone(), 1024).unwrap();
    services.prepare().unwrap();
    let limits = ManagedStagingLimits {
        max_bytes: 128,
        budget: services.staging_budget(),
        worker: crate::command_execution::MimeExecutionWorker::new(port),
    };
    let read_tree = services.new_read_tree(128);
    let body_read_tree = Arc::clone(&read_tree);
    let input = OwnedMimeInput::bytes(Arc::from(b"owned staging payload".as_slice()), InputExtent::Complete);
    let mut handle = block_on(group.try_submit_async(Default::default(), move |scope| async move {
        stage_owned_in(&scope, input, limits, body_read_tree).await
    }))
    .unwrap();
    let resource = match block_on(handle.wait_outcome()).unwrap() {
        qubit_operation::OperationOutcome::Succeeded(resource) => resource,
        _ => panic!("staging operation did not succeed"),
    };
    let path = resource.path().unwrap();
    assert!(
        path.exists(),
        "resource ref keeps the staged file alive after body exit"
    );
    drop(resource);
    let report = block_on(handle.wait_settlement());
    assert!(report.all_released());
    assert!(!path.exists(), "scope end plus final reference drop triggers cleanup");
    let shutdown = services.shutdown();
    let _ = block_on(shutdown.wait_services_settled()).unwrap();
    let _ = shutdown.stop_services().unwrap();
}

#[test]
fn test_managed_staging_open_scope_does_not_clean_after_last_reference_drops() {
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let group = manager.group(qubit_operation::GroupOptions { capacity: 4 }).unwrap();
    let port = Arc::new(FixedMimeExecutionPort::new(1, 4));
    let services = MimeExecutionServices::new(manager, group.clone(), port.clone(), 1024).unwrap();
    services.prepare().unwrap();
    let limits = ManagedStagingLimits {
        max_bytes: 128,
        budget: services.staging_budget(),
        worker: MimeExecutionWorker::new(port),
    };
    let read_tree = services.new_read_tree(128);
    let body_read_tree = Arc::clone(&read_tree);
    let input = OwnedMimeInput::bytes(Arc::from(b"scope still open".as_slice()), InputExtent::Complete);
    let path_slot = Arc::new(std::sync::Mutex::new(None));
    let body_path_slot = Arc::clone(&path_slot);
    let scope_open = Arc::new(Barrier::new(2));
    let body_scope_open = Arc::clone(&scope_open);
    let scope_release = Arc::new(Barrier::new(2));
    let body_scope_release = Arc::clone(&scope_release);
    let mut handle = block_on(group.try_submit_async(Default::default(), move |scope| async move {
        let staged = stage_owned_in(&scope, input, limits, body_read_tree).await?;
        *body_path_slot.lock().unwrap() = Some(staged.path()?);
        drop(staged);

        // Keep the operation scope open after its final resource reference is
        // gone so cleanup cannot be mistaken for an ordinary reference drop.
        body_scope_open.wait();
        body_scope_release.wait();
        Ok::<(), crate::MimeError>(())
    }))
    .unwrap();

    scope_open.wait();
    let path = path_slot.lock().unwrap().as_ref().unwrap().clone();
    assert!(path.exists(), "an open scope retains the registered temp owner");
    assert_eq!(16, services.staging_budget().used_bytes());
    scope_release.wait();
    assert!(matches!(
        block_on(handle.wait_outcome()).unwrap(),
        qubit_operation::OperationOutcome::Succeeded(())
    ));
    assert!(block_on(handle.wait_settlement()).all_released());
    assert!(!path.exists(), "scope end triggers cleanup after references are gone");
    assert_eq!(0, services.staging_budget().used_bytes());
    let shutdown = services.shutdown();
    let _ = block_on(shutdown.wait_services_settled()).unwrap();
    let _ = shutdown.stop_services().unwrap();
}

#[test]
fn test_managed_staging_parent_can_wait_for_child_settlement_while_holding_resource() {
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let group = manager.group(qubit_operation::GroupOptions { capacity: 4 }).unwrap();
    let port = Arc::new(FixedMimeExecutionPort::new(1, 4));
    let services = MimeExecutionServices::new(manager, group.clone(), port.clone(), 1024).unwrap();
    services.prepare().unwrap();
    let limits = ManagedStagingLimits {
        max_bytes: 128,
        budget: services.staging_budget(),
        worker: MimeExecutionWorker::new(port),
    };
    let read_tree = services.new_read_tree(128);
    let body_read_tree = Arc::clone(&read_tree);
    let input = OwnedMimeInput::bytes(Arc::from(b"parent held resource".as_slice()), InputExtent::Complete);
    let child_entered = Arc::new(Barrier::new(2));
    let child_release = Arc::new(Barrier::new(2));
    let body_child_entered = Arc::clone(&child_entered);
    let body_child_release = Arc::clone(&child_release);
    let mut handle = block_on(group.try_submit_async(Default::default(), move |scope| async move {
        let staged = stage_owned_in(&scope, input, limits, body_read_tree).await?;
        let path = staged.path()?;
        let child_resource = staged.clone();
        let child_entered = Arc::clone(&body_child_entered);
        let child_release = Arc::clone(&body_child_release);
        let child = scope
            .manager()
            .as_async()
            .try_submit(Default::default(), move |_child_scope| async move {
                child_entered.wait();
                child_release.wait();
                drop(child_resource);
                Ok::<(), crate::MimeError>(())
            })
            .unwrap();

        // Barriers explicitly order child execution and release while the
        // parent continues to hold its resource reference.
        let release_entered = body_child_entered;
        let release = body_child_release;
        let release_thread = std::thread::spawn(move || {
            release_entered.wait();
            release.wait();
        });
        let child_report = child.wait_settlement().await;
        release_thread.join().unwrap();
        assert!(child_report.all_released());
        assert!(path.exists(), "the parent reference keeps its resource alive");
        Ok::<_, crate::MimeError>(staged)
    }))
    .unwrap();
    let resource = match block_on(handle.wait_outcome()).unwrap() {
        qubit_operation::OperationOutcome::Succeeded(resource) => resource,
        _ => panic!("parent operation did not return its managed resource"),
    };
    let path = resource.path().unwrap();
    assert!(path.exists());
    drop(resource);
    assert!(block_on(handle.wait_settlement()).all_released());
    assert!(
        !path.exists(),
        "the parent resource is cleaned after its final reference drops"
    );
    let shutdown = services.shutdown();
    let _ = block_on(shutdown.wait_services_settled()).unwrap();
    let _ = shutdown.stop_services().unwrap();
}

/// Executor that retains its input after returning a successful outcome.
struct EarlyOutcomeCommand {
    helper_input:
        Arc<std::sync::Mutex<Option<qubit_operation::ResourceRef<crate::managed_temp_staging::StagedMimeInput>>>>,
}
impl std::fmt::Debug for EarlyOutcomeCommand {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("EarlyOutcomeCommand")
    }
}

impl AsyncMimeCommandExecutor for EarlyOutcomeCommand {
    fn run_in<'a>(
        &'a self,
        _scope: &'a qubit_operation::OperationScope,
        _command: ManagedMimeCommand,
        input: qubit_operation::ResourceRef<crate::managed_temp_staging::StagedMimeInput>,
    ) -> crate::MimeFuture<'a, MimeCommandOutput> {
        *self.helper_input.lock().unwrap() = Some(input);
        Box::pin(async {
            Ok(MimeCommandOutput {
                exit_code: Some(0),
                stdout: Vec::new(),
                stdout_truncated: false,
                stdout_complete: true,
            })
        })
    }
}

#[test]
fn test_managed_staging_outcome_precedes_helper_input_release() {
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let group = manager.group(qubit_operation::GroupOptions { capacity: 4 }).unwrap();
    let port = Arc::new(FixedMimeExecutionPort::new(1, 4));
    let services = MimeExecutionServices::new(manager, group.clone(), port.clone(), 1024).unwrap();
    services.prepare().unwrap();
    let limits = ManagedStagingLimits {
        max_bytes: 128,
        budget: services.staging_budget(),
        worker: MimeExecutionWorker::new(port),
    };
    let read_tree = services.new_read_tree(128);
    let body_read_tree = Arc::clone(&read_tree);
    let helper_input = Arc::new(std::sync::Mutex::new(None));
    let executor = Arc::new(EarlyOutcomeCommand {
        helper_input: Arc::clone(&helper_input),
    });
    let command_group = group.clone();
    let input = OwnedMimeInput::bytes(Arc::from(b"early result".as_slice()), InputExtent::Complete);
    let mut handle = block_on(group.try_submit_async(Default::default(), move |scope| async move {
        let staged = stage_owned_in(&scope, input, limits, body_read_tree).await?;
        let request = ManagedMimeCommand::new(
            qubit_command::CommandRunner::new(std::time::Duration::from_secs(1)),
            qubit_command::Command::new("unused-by-fake"),
            command_group,
        );
        executor.run_in(&scope, request, staged).await
    }))
    .unwrap();
    assert!(matches!(
        block_on(handle.wait_outcome()).unwrap(),
        qubit_operation::OperationOutcome::Succeeded(_)
    ));
    let held = helper_input.lock().unwrap().as_ref().unwrap().clone();
    let path = held.path().unwrap();
    assert!(
        path.exists(),
        "the outcome does not release an input still held by a helper"
    );
    drop(held);
    helper_input.lock().unwrap().take();
    block_on(handle.wait_settlement());
    assert!(
        !path.exists(),
        "the temp is cleaned after the helper returns its final reference"
    );
    let shutdown = services.shutdown();
    let _ = block_on(shutdown.wait_services_settled()).unwrap();
    let _ = shutdown.stop_services().unwrap();
}

#[cfg(unix)]
#[test]
fn test_managed_staging_cleanup_failure_retains_owner_for_retry() {
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let group = manager.group(qubit_operation::GroupOptions { capacity: 4 }).unwrap();
    let port = Arc::new(FixedMimeExecutionPort::new(1, 4));
    let services = MimeExecutionServices::new(manager, group.clone(), port.clone(), 1024).unwrap();
    services.prepare().unwrap();
    let limits = ManagedStagingLimits {
        max_bytes: 128,
        budget: services.staging_budget(),
        worker: MimeExecutionWorker::new(port),
    };
    let read_tree = services.new_read_tree(128);
    let body_read_tree = Arc::clone(&read_tree);
    let input = OwnedMimeInput::bytes(Arc::from(b"retry cleanup".as_slice()), InputExtent::Complete);
    let mut handle = block_on(group.try_submit_async(Default::default(), move |scope| async move {
        stage_owned_in(&scope, input, limits, body_read_tree).await
    }))
    .unwrap();
    let resource = match block_on(handle.wait_outcome()).unwrap() {
        qubit_operation::OperationOutcome::Succeeded(resource) => resource,
        _ => panic!("staging operation did not succeed"),
    };
    let path = resource.path().unwrap();
    let sandbox = path.parent().unwrap().to_path_buf();
    let blocker = sandbox.join("cleanup-blocker");
    std::fs::write(&blocker, b"keep sandbox nonempty").unwrap();
    drop(resource);

    let report = block_on(handle.wait_settlement());
    assert_eq!(report.status, qubit_operation::SettlementStatus::RecoveryRequired);
    assert!(report.recoveries.len() == 1, "failed cleanup keeps one recovery owner");
    assert_eq!(
        13,
        services.staging_budget().used_bytes(),
        "failed cleanup keeps staging capacity reserved with the owner"
    );
    assert!(
        !path.exists(),
        "cleanup removed the file but retained its sandbox owner"
    );
    std::fs::remove_file(blocker).unwrap();
    let recovery = report.recoveries[0].clone();
    let attempt = recovery.try_retry().unwrap();
    let retry_report = block_on(attempt.wait_async());
    assert!(
        retry_report.failures.is_empty(),
        "retry should release the retained sandbox owner"
    );
    assert_eq!(
        0,
        services.staging_budget().used_bytes(),
        "successful cleanup retry releases staged-byte capacity"
    );
    assert!(!sandbox.exists(), "retry removes the retained sandbox");
    let shutdown = services.shutdown();
    let _ = block_on(shutdown.wait_services_settled()).unwrap();
    let _ = shutdown.stop_services().unwrap();
}

#[test]
fn test_managed_staging_commit_failure_retains_partial_owner_and_permit() {
    let _recovery_port_guard = recovery_cleanup_test_guard();

    #[derive(Debug)]
    struct RejectingCleanupPort;
    impl MimeExecutionPort for RejectingCleanupPort {
        fn prepare(&self) -> Result<(), MimeError> {
            Ok(())
        }
        fn try_dispatch(
            &self,
            job: Box<dyn FnOnce() + Send + 'static>,
        ) -> Result<(), Box<dyn FnOnce() + Send + 'static>> {
            Err(job)
        }
        fn shutdown(&self) -> Result<(), MimeError> {
            Ok(())
        }
    }

    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let group = manager.group(qubit_operation::GroupOptions { capacity: 4 }).unwrap();
    let port = Arc::new(FixedMimeExecutionPort::new(1, 4));
    let services = MimeExecutionServices::new(manager, group.clone(), port.clone(), 1024).unwrap();
    services.prepare().unwrap();
    let worker = MimeExecutionWorker::new(Arc::new(RejectingCleanupPort));
    let reservation_slot = Arc::new(std::sync::Mutex::new(None));
    let slot = Arc::clone(&reservation_slot);
    let mut handle = block_on(group.try_submit_async(Default::default(), move |scope| async move {
        *slot.lock().unwrap() = Some(scope.reserve_resource().unwrap());
        Ok::<(), crate::MimeError>(())
    }))
    .unwrap();
    assert!(matches!(
        block_on(handle.wait_outcome()).unwrap(),
        qubit_operation::OperationOutcome::Succeeded(())
    ));

    let mut temp = qubit_local_files::LocalFileSystem::host()
        .unwrap()
        .create_temp_file()
        .unwrap();
    temp.as_file_mut().unwrap().write_all(b"partial owner").unwrap();
    let path = temp.path().to_path_buf();
    let permit = services.staging_budget().try_reserve(13).unwrap();
    let staged = StagedMimeInput::new(temp, permit, worker);
    let reservation = reservation_slot.lock().unwrap().take().unwrap();
    let (hook, event_rx, pending_release_tx, publish_release_tx) = controlled_identity_hook();
    staged.set_cleanup_test_hook(hook);
    let (commit_tx, commit_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || commit_tx.send(commit_staged(reservation, staged)).unwrap());

    let published_id = wait_for_publication_and_pending_cleanup(&event_rx);
    let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    {
        let mut settlement = std::pin::pin!(handle.wait_settlement());
        assert!(settlement.as_mut().poll(&mut context).is_pending());
    }
    assert!(path.exists(), "early cleanup poll keeps the real partial owner alive");
    assert_eq!(13, services.staging_budget().used_bytes());
    pending_release_tx.send(()).unwrap();
    publish_release_tx.send(()).unwrap();
    assert!(matches!(
        commit_rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap(),
        Err(crate::MimeError::ExecutionServicesRequired)
    ));
    let report = block_on(handle.wait_settlement());
    assert_eq!(qubit_operation::SettlementStatus::RecoveryRequired, report.status);
    assert_eq!(1, report.cleanup_failures.len());
    assert_eq!(published_id, report.cleanup_failures[0].resource_id);
    assert_eq!([published_id], report.recoveries[0].snapshot().resources.as_ref());
    assert!(
        path.exists(),
        "failed cleanup retains the partial temp until recovery retry"
    );
    assert_eq!(13, services.staging_budget().used_bytes());
    let retry = report.recoveries[0].try_retry().unwrap();
    let retry_report = block_on(retry.wait_async());
    assert!(
        retry_report.failures.is_empty(),
        "retry failures: {:#?}",
        retry_report.failures
    );
    assert_eq!(0, services.staging_budget().used_bytes());
    assert!(!path.exists());
    let shutdown = services.shutdown();
    let _ = block_on(shutdown.wait_services_settled()).unwrap();
    let _ = shutdown.stop_services().unwrap();
}

#[test]
fn test_managed_staging_commit_failure_successful_cleanup_releases_permit() {
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let group = manager.group(qubit_operation::GroupOptions { capacity: 2 }).unwrap();
    let port = Arc::new(FixedMimeExecutionPort::new(1, 2));
    let services = MimeExecutionServices::new(manager, group.clone(), port.clone(), 1024).unwrap();
    services.prepare().unwrap();
    let reservation_slot = Arc::new(std::sync::Mutex::new(None));
    let slot = Arc::clone(&reservation_slot);
    let mut handle = block_on(group.try_submit_async(Default::default(), move |scope| async move {
        *slot.lock().unwrap() = Some(scope.reserve_resource().unwrap());
        Ok::<(), MimeError>(())
    }))
    .unwrap();
    assert!(matches!(
        block_on(handle.wait_outcome()).unwrap(),
        qubit_operation::OperationOutcome::Succeeded(())
    ));
    let temp = LocalFileSystem::host().unwrap().create_temp_file().unwrap();
    let path = temp.path().to_path_buf();
    let permit = services.staging_budget().try_reserve(13).unwrap();
    let staged = StagedMimeInput::new(temp, permit, MimeExecutionWorker::new(port));
    let reservation = reservation_slot.lock().unwrap().take().unwrap();
    assert!(matches!(
        commit_staged(reservation, staged),
        Err(MimeError::ExecutionServicesRequired)
    ));
    let report = block_on(handle.wait_settlement());
    assert!(report.cleanup_failures.is_empty());
    assert!(!path.exists());
    assert_eq!(0, services.staging_budget().used_bytes());
    let shutdown = services.shutdown();
    let _ = block_on(shutdown.wait_services_settled()).unwrap();
    let _ = shutdown.stop_services().unwrap();
}

#[cfg(unix)]
#[test]
fn test_managed_staging_creation_recovery_is_settled_and_retryable() {
    let _recovery_port_guard = recovery_cleanup_test_guard();

    let (hook, event_rx, pending_release_tx, publish_release_tx) = controlled_identity_hook();
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let group = manager.group(qubit_operation::GroupOptions { capacity: 2 }).unwrap();
    let port = Arc::new(FixedMimeExecutionPort::new(1, 2));
    let services = MimeExecutionServices::new(manager, group.clone(), port.clone(), 1024).unwrap();
    services.prepare().unwrap();
    let worker = MimeExecutionWorker::new(Arc::new(RejectCleanupAfterFirstDispatch {
        inner: Arc::clone(&port),
        dispatches: AtomicUsize::new(0),
    }));
    let limits = ManagedStagingLimits {
        max_bytes: 128,
        budget: services.staging_budget(),
        worker,
    };
    let blocker_path = Arc::new(std::sync::Mutex::new(None));
    let blocker_path_for_create = Arc::clone(&blocker_path);
    let recovery_owner_path = Arc::new(std::sync::Mutex::new(None));
    let recovery_owner_path_for_create = Arc::clone(&recovery_owner_path);
    let mut handle = block_on(group.try_submit_async(Default::default(), move |scope| async move {
        let reservation = scope.reserve_resource().unwrap();
        create_empty_staged_with_hook(&scope, reservation, &limits, Some(hook), move || {
            let recovery_fault =
                qubit_local_files::test_support::install_test_fault("host-temp-file-recovery-clone-cleanup")
                    .expect("creation and initial cleanup fault should install");
            let result = LocalFileSystem::host()?
                .create_temp_file_with_options(
                    &LocalTempFileOptions::new().with_privacy(LocalPrivacyRequirement::PlatformDefault),
                )
                .map_err(Box::new);
            if let Err(error) = &result
                && let Some(recovery) = error.temp_recovery()
            {
                *recovery_owner_path_for_create.lock().unwrap() = Some(recovery.path().to_path_buf());
                let blocker = recovery.path().parent().unwrap().join("recovery-blocker");
                std::fs::write(&blocker, b"hold recovery owner").unwrap();
                *blocker_path_for_create.lock().unwrap() = Some(blocker);
            }
            drop(recovery_fault);
            result
        })
        .await
    }))
    .unwrap();

    let published_id = wait_for_publication_and_pending_cleanup(&event_rx);
    let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    {
        let mut outcome = std::pin::pin!(handle.wait_outcome());
        assert!(outcome.as_mut().poll(&mut context).is_pending());
    }
    let owner_path = recovery_owner_path
        .lock()
        .unwrap()
        .clone()
        .expect("creation recovery owner path");
    let blocker = blocker_path
        .lock()
        .unwrap()
        .clone()
        .expect("creation recovery blocker path");
    assert!(owner_path.exists(), "owner remains live during early cleanup polling");
    assert!(blocker.exists(), "blocker remains live during early cleanup polling");
    pending_release_tx.send(()).unwrap();
    publish_release_tx.send(()).unwrap();

    match block_on(handle.wait_outcome()).unwrap() {
        qubit_operation::OperationOutcome::Failed(MimeError::Io(_)) => {}
        qubit_operation::OperationOutcome::Succeeded(_) => panic!("creation fault unexpectedly succeeded"),
        qubit_operation::OperationOutcome::Failed(_) => panic!("creation failure should retain its I/O cause"),
        _ => panic!("creation fault produced an unexpected operation outcome"),
    }
    assert!(
        format!("{port:?}").contains("prepared: true"),
        "cleanup remains dispatchable while recovery is unresolved"
    );
    let report = block_on(handle.wait_settlement());
    assert_eq!(report.status, qubit_operation::SettlementStatus::RecoveryRequired);
    assert_eq!(1, report.cleanup_failures.len());
    assert_eq!(published_id, report.cleanup_failures[0].resource_id);
    assert_eq!(
        1,
        report.recoveries.len(),
        "settlement retains the creation recovery owner"
    );
    assert_eq!([published_id], report.recoveries[0].snapshot().resources.as_ref());
    assert!(owner_path.exists(), "rejected cleanup retains the owner after failure");
    assert!(blocker.exists(), "the creation recovery blocker remains until retry");

    std::fs::remove_file(blocker).unwrap();
    let recovery = report.recoveries[0].clone();
    let attempt = recovery.try_retry().unwrap();
    let retry_report = block_on(attempt.wait_async());
    assert!(
        retry_report.failures.is_empty(),
        "retry releases the recovery owner; failures: {:#?}",
        retry_report.failures
    );
    assert!(
        format!("{port:?}").contains("prepared: true"),
        "the execution port stays available until cleanup resolves"
    );

    let shutdown = services.shutdown();
    let _ = block_on(shutdown.wait_services_settled()).unwrap();
    let _ = shutdown.stop_services().unwrap();
}

#[test]
fn test_managed_staging_creation_error_without_recovery_keeps_source_and_releases_reservation() {
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let group = manager.group(qubit_operation::GroupOptions { capacity: 2 }).unwrap();
    let port = Arc::new(FixedMimeExecutionPort::new(1, 2));
    let services = MimeExecutionServices::new(manager, group.clone(), port.clone(), 1024).unwrap();
    services.prepare().unwrap();
    let limits = ManagedStagingLimits {
        max_bytes: 128,
        budget: services.staging_budget(),
        worker: MimeExecutionWorker::new(port),
    };
    let mut handle = block_on(group.try_submit_async(Default::default(), move |scope| async move {
        let reservation = scope.reserve_resource().unwrap();
        create_empty_staged_with(&scope, reservation, &limits, || {
            LocalFileSystem::host()?
                .create_temp_file_with_options(
                    &LocalTempFileOptions::new()
                        .with_privacy(LocalPrivacyRequirement::PlatformDefault)
                        .with_max_attempts(0),
                )
                .map_err(Box::new)
        })
        .await
    }))
    .unwrap();
    match block_on(handle.wait_outcome()).unwrap() {
        qubit_operation::OperationOutcome::Failed(MimeError::Io(error)) => assert!(
            error
                .get_ref()
                .is_some_and(|source| source.is::<qubit_local_files::LocalFileError>()),
            "the structured local-files error remains the typed I/O source"
        ),
        qubit_operation::OperationOutcome::Succeeded(_) => panic!("invalid create options unexpectedly succeeded"),
        _ => panic!("invalid create options produced an unexpected outcome"),
    }
    let report = block_on(handle.wait_settlement());
    assert_eq!(report.status, qubit_operation::SettlementStatus::Released);
    assert!(
        report.recoveries.is_empty(),
        "ordinary creation failure releases its reservation"
    );
    let shutdown = services.shutdown();
    let _ = block_on(shutdown.wait_services_settled()).unwrap();
    let _ = shutdown.stop_services().unwrap();
}

#[test]
fn test_managed_staging_unconsumed_outcome_does_not_hold_temp() {
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let group = manager.group(qubit_operation::GroupOptions { capacity: 4 }).unwrap();
    let port = Arc::new(FixedMimeExecutionPort::new(1, 4));
    let services = MimeExecutionServices::new(manager, group.clone(), port.clone(), 1024).unwrap();
    services.prepare().unwrap();
    let limits = ManagedStagingLimits {
        max_bytes: 128,
        budget: services.staging_budget(),
        worker: MimeExecutionWorker::new(port),
    };
    let path_slot = Arc::new(std::sync::Mutex::new(None));
    let slot = Arc::clone(&path_slot);
    let input = OwnedMimeInput::bytes(Arc::from(b"no resource in outcome".as_slice()), InputExtent::Complete);
    let read_tree = services.new_read_tree(128);
    let body_read_tree = Arc::clone(&read_tree);
    let handle = block_on(group.try_submit_async(Default::default(), move |scope| async move {
        let staged = stage_owned_in(&scope, input, limits, body_read_tree).await?;
        *slot.lock().unwrap() = Some(staged.path()?);
        drop(staged);
        Ok::<(), crate::MimeError>(())
    }))
    .unwrap();
    // Deliberately never consume the unit outcome. It contains no resource ref.
    let report = block_on(handle.wait_settlement());
    assert!(report.all_released());
    assert!(!path_slot.lock().unwrap().as_ref().unwrap().exists());
    let shutdown = services.shutdown();
    let _ = block_on(shutdown.wait_services_settled()).unwrap();
    assert!(shutdown.snapshot().operations_settled);
    assert_eq!(0, shutdown.snapshot().unresolved_recoveries);
    assert!(group.snapshot().active_operations > 0, "outcome stays caller-owned");
    let _ = shutdown.stop_services().unwrap();
}
