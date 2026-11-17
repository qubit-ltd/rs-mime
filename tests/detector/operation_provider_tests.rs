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
use std::sync::Mutex;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::task::Context;
use std::task::Poll;
use std::task::Wake;
use std::task::Waker;

use qubit_mime::AsyncMediaStreamClassifierBackend;
use qubit_mime::AsyncMediaStreamClassifierProvider;
use qubit_mime::AsyncMimeDetectorBackend;
use qubit_mime::AsyncMimeDetectorProvider;
use qubit_mime::BorrowedMimeInput;
use qubit_mime::ContentRequirement;
use qubit_mime::FixedMimeExecutionPort;
use qubit_mime::InputExtent;
use qubit_mime::InspectedExtent;
use qubit_mime::MediaStreamClassifier;
use qubit_mime::MediaStreamClassifierRegistry;
use qubit_mime::MediaStreamType;
use qubit_mime::MimeCandidate;
use qubit_mime::MimeCandidates;
use qubit_mime::MimeCapability;
use qubit_mime::MimeConfig;
use qubit_mime::MimeDetectionPolicy;
use qubit_mime::MimeDetector;
use qubit_mime::MimeDetectorBackend;
use qubit_mime::MimeDetectorCore;
use qubit_mime::MimeDetectorRegistry;
use qubit_mime::MimeError;
use qubit_mime::MimeExecutionServices;
use qubit_mime::MimeFuture;
use qubit_mime::MimeOperationOptions;
use qubit_mime::MimeRepository;
use qubit_mime::OwnedMimeInput;
use qubit_mime::RandomAccessCapability;
use qubit_mime::detector::MimeRuntime;
use qubit_spi::ProviderDescriptor;
use qubit_spi::ProviderId;
use qubit_spi::ProviderMetadata;
use qubit_spi::ServiceProvider;
use qubit_spi::error::ProviderFailure;
use qubit_spi::provider_descriptor;

#[test]
fn test_operation_provider_same_object_and_path_replacement_uses_open_snapshot() {
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let group = manager.group(qubit_operation::GroupOptions { capacity: 2 }).unwrap();
    let services =
        MimeExecutionServices::new(manager, group, Arc::new(FixedMimeExecutionPort::new(1, 2)), 1024).unwrap();
    services.prepare().unwrap();

    let content = Arc::new(Mutex::new(b"%PDF".to_vec()));
    let opens = Arc::new(AtomicUsize::new(0));
    let (read_started_tx, read_started_rx) = mpsc::channel();
    let released = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let read_waker = Arc::new(Mutex::new(None));
    let file_system = qubit_fs::AsyncFileSystem::from_spi(SnapshotGateFileSystem::new(
        Arc::clone(&content),
        Arc::clone(&opens),
        read_started_tx,
        Arc::clone(&released),
        Arc::clone(&read_waker),
        None,
    ))
    .unwrap();

    let registry = MimeDetectorRegistry::default();
    registry.register_async_provider(SnapshotProvider).unwrap();
    let runtime = MimeRuntime::builtin()
        .unwrap()
        .with_execution_services(services.clone())
        .with_async_provider_selection(&registry, &qubit_spi::ProviderSelection::named("snapshot").unwrap())
        .unwrap();
    let mut operation = runtime
        .try_submit_detection(
            OwnedMimeInput::Provider {
                file_system: Arc::new(file_system),
                path: qubit_fs::Path::parse("/input").unwrap(),
            },
            MimeOperationOptions::default(),
        )
        .unwrap_or_else(|_| panic!("prepared snapshot operation should be admitted"));

    read_started_rx
        .recv()
        .expect("provider read should reach its deterministic gate");
    *content.lock().unwrap() = b"NOPE".to_vec();
    released.store(true, Ordering::SeqCst);
    read_waker
        .lock()
        .unwrap()
        .take()
        .expect("pending provider read should register a waker")
        .wake();

    match block_on(operation.wait_outcome()).unwrap() {
        qubit_operation::OperationOutcome::Succeeded(report) => {
            assert_eq!(report.mime_type.as_deref(), Some("application/x-snapshot"));
            assert_eq!(report.budget.bytes_read, 4);
        }
        other => panic!("operation should inspect the already-open object: {other:?}"),
    }
    assert_eq!(opens.load(Ordering::SeqCst), 1, "replacement must not reopen the path");
    let shutdown = services.shutdown();
    assert!(block_on(shutdown.wait_services_settled()).is_ok());
    assert!(shutdown.stop_services().is_ok());
}

#[test]
fn test_operation_provider_driver_loss_is_pending() {
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let group = manager.group(qubit_operation::GroupOptions { capacity: 2 }).unwrap();
    let services =
        MimeExecutionServices::new(manager, group, Arc::new(FixedMimeExecutionPort::new(1, 2)), 1024).unwrap();
    services.prepare().unwrap();

    let content = Arc::new(Mutex::new(b"%PDF".to_vec()));
    let opens = Arc::new(AtomicUsize::new(0));
    let (read_started_tx, read_started_rx) = mpsc::channel();
    let released = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let owner_dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let read_waker = Arc::new(Mutex::new(None));
    let file_system = qubit_fs::AsyncFileSystem::from_spi(SnapshotGateFileSystem::new(
        content,
        opens,
        read_started_tx,
        Arc::clone(&released),
        Arc::clone(&read_waker),
        Some(Arc::clone(&owner_dropped)),
    ))
    .unwrap();

    let registry = MimeDetectorRegistry::default();
    registry.register_async_provider(SnapshotProvider).unwrap();
    let runtime = MimeRuntime::builtin()
        .unwrap()
        .with_execution_services(services.clone())
        .with_async_provider_selection(&registry, &qubit_spi::ProviderSelection::named("snapshot").unwrap())
        .unwrap();
    let operation = runtime
        .try_submit_detection(
            OwnedMimeInput::Provider {
                file_system: Arc::new(file_system),
                path: qubit_fs::Path::parse("/input").unwrap(),
            },
            MimeOperationOptions::default(),
        )
        .unwrap_or_else(|_| panic!("prepared provider operation should be admitted"));
    let observer = operation.observer().as_sync();

    read_started_rx
        .recv()
        .expect("provider read should reach its deterministic gate");
    drop(operation);

    let snapshot = observer.snapshot();
    assert!(snapshot.settlement.is_none(), "settlement must remain pending");
    assert!(
        !owner_dropped.load(Ordering::SeqCst),
        "the provider reader owner must survive loss of the caller handle"
    );

    released.store(true, Ordering::SeqCst);
    read_waker
        .lock()
        .unwrap()
        .take()
        .expect("pending provider read should register a waker")
        .wake();
    observer
        .wait_settlement()
        .expect("observer should see worker settlement");
    assert!(
        owner_dropped.load(Ordering::SeqCst),
        "the provider reader owner should be released after worker settlement"
    );

    let shutdown = services.shutdown();
    assert!(block_on(shutdown.wait_services_settled()).is_ok());
    assert!(shutdown.stop_services().is_ok());
}

#[test]
fn test_operation_provider_random_access_backend_preserves_sample_extent() {
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let group = manager.group(qubit_operation::GroupOptions { capacity: 2 }).unwrap();
    let services =
        MimeExecutionServices::new(manager, group, Arc::new(FixedMimeExecutionPort::new(1, 2)), 1024).unwrap();
    services.prepare().unwrap();

    let content = Arc::new(Mutex::new(b"sample-object".to_vec()));
    let opens = Arc::new(AtomicUsize::new(0));
    let (read_started, _read_started_rx) = mpsc::channel();
    let released = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let read_waker = Arc::new(Mutex::new(None));
    let file_system = qubit_fs::AsyncFileSystem::from_spi(SnapshotGateFileSystem::new(
        content,
        Arc::clone(&opens),
        read_started,
        released,
        read_waker,
        None,
    ))
    .unwrap();

    let registry = MimeDetectorRegistry::default();
    registry.register_async_provider(RandomAccessProvider).unwrap();
    let runtime = MimeRuntime::builtin()
        .unwrap()
        .with_execution_services(services.clone())
        .with_async_provider_selection(
            &registry,
            &qubit_spi::ProviderSelection::named("random-access").unwrap(),
        )
        .unwrap();
    let mut operation = runtime
        .try_submit_detection(
            OwnedMimeInput::Provider {
                file_system: Arc::new(file_system),
                path: qubit_fs::Path::parse("/sample").unwrap(),
            },
            MimeOperationOptions::default(),
        )
        .unwrap_or_else(|_| panic!("random-access provider operation should be admitted"));

    match block_on(operation.wait_outcome()).unwrap() {
        qubit_operation::OperationOutcome::Succeeded(report) => {
            assert_eq!(report.mime_type.as_deref(), Some("application/x-sampled"));
            assert_eq!(report.inspected_extent, InspectedExtent::RandomAccessSamples);
        }
        other => panic!("random-access backend should report its sample extent: {other:?}"),
    }
    assert_eq!(
        opens.load(Ordering::SeqCst),
        0,
        "the runtime must not flatten random-access input to a prefix"
    );
    let shutdown = services.shutdown();
    assert!(block_on(shutdown.wait_services_settled()).is_ok());
    assert!(shutdown.stop_services().is_ok());
}

#[derive(Debug)]
struct SnapshotProvider;

impl ProviderMetadata for SnapshotProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor::new(ProviderId::new("snapshot").unwrap())
    }
}

impl AsyncMimeDetectorProvider for SnapshotProvider {
    fn create_async_backend(
        &self,
        _config: &MimeConfig,
        _services: &MimeExecutionServices,
    ) -> Result<Arc<dyn AsyncMimeDetectorBackend>, MimeError> {
        Ok(Arc::new(SnapshotBackend))
    }
}

#[derive(Debug)]
struct SnapshotBackend;

impl AsyncMimeDetectorBackend for SnapshotBackend {
    fn content_requirement(&self) -> ContentRequirement {
        ContentRequirement::Prefix(4)
    }

    fn random_access_capability(&self) -> RandomAccessCapability {
        RandomAccessCapability::Unsupported
    }

    fn guess_content_in<'a>(
        &'a self,
        _scope: &'a qubit_operation::OperationScope,
        input: BorrowedMimeInput<'a>,
        _options: &'a MimeOperationOptions,
    ) -> MimeFuture<'a, MimeCandidates> {
        Box::pin(async move {
            let content = match input {
                BorrowedMimeInput::Bytes { content, .. } => content,
                BorrowedMimeInput::Provider { .. } => {
                    panic!("prefix input should be read once by runtime")
                }
            };
            assert_eq!(content, b"%PDF", "the opened reader must retain its original snapshot");
            Ok(MimeCandidates {
                candidates: vec![MimeCandidate {
                    mime_type: "application/x-snapshot".into(),
                    rank: 0,
                }],
                inspected_extent: InspectedExtent::Prefix(content.len()),
                retained_prefix: content.to_vec(),
                staging_bytes: 0,
            })
        })
    }
}

#[derive(Debug)]
struct RandomAccessProvider;

impl ProviderMetadata for RandomAccessProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor::new(ProviderId::new("random-access").unwrap())
    }
}

impl AsyncMimeDetectorProvider for RandomAccessProvider {
    fn create_async_backend(
        &self,
        _config: &MimeConfig,
        _services: &MimeExecutionServices,
    ) -> Result<Arc<dyn AsyncMimeDetectorBackend>, MimeError> {
        Ok(Arc::new(RandomAccessBackend))
    }
}

#[derive(Debug)]
struct RandomAccessBackend;

impl AsyncMimeDetectorBackend for RandomAccessBackend {
    fn content_requirement(&self) -> ContentRequirement {
        ContentRequirement::Prefix(1)
    }

    fn random_access_capability(&self) -> RandomAccessCapability {
        RandomAccessCapability::SamplesSameObject
    }

    fn guess_content_in<'a>(
        &'a self,
        _scope: &'a qubit_operation::OperationScope,
        input: BorrowedMimeInput<'a>,
        _options: &'a MimeOperationOptions,
    ) -> MimeFuture<'a, MimeCandidates> {
        Box::pin(async move {
            let BorrowedMimeInput::Provider { file_system, path } = input else {
                panic!("random-access provider must receive its provider input directly")
            };
            let _ = (file_system, path);
            Ok(MimeCandidates {
                candidates: vec![MimeCandidate {
                    mime_type: "application/x-sampled".into(),
                    rank: 0,
                }],
                inspected_extent: InspectedExtent::RandomAccessSamples,
                retained_prefix: Vec::new(),
                staging_bytes: 0,
            })
        })
    }
}

struct SnapshotGateFileSystem {
    properties: qubit_fs::spi::ProviderProperties,
    content: Arc<Mutex<Vec<u8>>>,
    opens: Arc<AtomicUsize>,
    read_started: mpsc::Sender<()>,
    released: Arc<std::sync::atomic::AtomicBool>,
    read_waker: Arc<Mutex<Option<Waker>>>,
    owner_dropped: Option<Arc<std::sync::atomic::AtomicBool>>,
}

impl SnapshotGateFileSystem {
    fn new(
        content: Arc<Mutex<Vec<u8>>>,
        opens: Arc<AtomicUsize>,
        read_started: mpsc::Sender<()>,
        released: Arc<std::sync::atomic::AtomicBool>,
        read_waker: Arc<Mutex<Option<Waker>>>,
        owner_dropped: Option<Arc<std::sync::atomic::AtomicBool>>,
    ) -> Self {
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
        let properties = qubit_fs::spi::ProviderProperties::new(
            FileSystemInfo::new(
                FileSystemId::new("mime-operation-snapshot").unwrap(),
                "mime-operation-snapshot",
                PathSemantics::Hierarchical,
            ),
            ProviderOperations::new().with(ProviderOperation::OpenReader),
            FileSystemCapabilities::new().with_guaranteed(FileSystemCapability::Read),
            FileSystemLimits::unknown(),
            PathConstraints::absolute(),
            SymlinkPolicy::Reject,
        )
        .unwrap();
        Self {
            properties,
            content,
            opens,
            read_started,
            released,
            read_waker,
            owner_dropped,
        }
    }
}

impl qubit_fs::spi::AsyncFileSystemSpi for SnapshotGateFileSystem {
    fn properties(&self) -> qubit_fs::spi::ProviderProperties {
        self.properties.clone()
    }

    fn stat<'a>(
        &'a self,
        _request: qubit_fs::spi::StatRequest<'a>,
    ) -> qubit_fs::spi::SpiFuture<'a, qubit_fs::FsResult<qubit_fs::spi::StatResponse>> {
        Box::pin(async { panic!("runtime prefix reading must not stat provider input") })
    }

    fn open_reader<'a>(
        &'a self,
        request: qubit_fs::spi::OpenReaderRequest<'a>,
    ) -> qubit_fs::spi::SpiFuture<'a, qubit_fs::FsResult<qubit_fs::spi::OpenedAsyncReader>> {
        self.opens.fetch_add(1, Ordering::SeqCst);
        let snapshot = self.content.lock().unwrap().clone();
        let info = qubit_fs::metadata::OpenedFileInfo::new(
            qubit_fs::metadata::FileSystemId::new("mime-operation-snapshot").unwrap(),
            request.path().clone(),
        );
        let input = SnapshotGateInput {
            bytes: snapshot,
            read_started: Some(self.read_started.clone()),
            released: Arc::clone(&self.released),
            read_waker: Arc::clone(&self.read_waker),
            owner_dropped: self.owner_dropped.as_ref().map(Arc::clone),
        };
        Box::pin(async move { Ok(qubit_fs::spi::OpenedAsyncReader::new(info, Box::new(input))) })
    }
}

struct SnapshotGateInput {
    bytes: Vec<u8>,
    read_started: Option<mpsc::Sender<()>>,
    released: Arc<std::sync::atomic::AtomicBool>,
    read_waker: Arc<Mutex<Option<Waker>>>,
    owner_dropped: Option<Arc<std::sync::atomic::AtomicBool>>,
}

impl Drop for SnapshotGateInput {
    fn drop(&mut self) {
        if let Some(owner_dropped) = &self.owner_dropped {
            owner_dropped.store(true, Ordering::SeqCst);
        }
    }
}

impl qubit_io::AsyncInput for SnapshotGateInput {
    type Item = u8;

    unsafe fn poll_read_unchecked(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        output: &mut [u8],
        index: usize,
        count: usize,
    ) -> Poll<std::io::Result<usize>> {
        let this = self.get_mut();
        if !this.released.load(Ordering::SeqCst) {
            *this.read_waker.lock().unwrap() = Some(context.waker().clone());
            if let Some(sender) = this.read_started.take() {
                sender.send(()).expect("test should observe the blocked read");
            }
            return Poll::Pending;
        }
        let length = count.min(this.bytes.len());
        output[index..index + length].copy_from_slice(&this.bytes[..length]);
        Poll::Ready(Ok(length))
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
    let mut cx = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    loop {
        match Pin::as_mut(&mut future).poll(&mut cx) {
            Poll::Ready(value) => return value,
            Poll::Pending => std::thread::park(),
        }
    }
}

#[test]
fn test_operation_provider_registration_is_pure() {
    let registry = MimeDetectorRegistry::default();
    let counts = Arc::new([AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0)]);
    registry.register(CountedProvider(Arc::clone(&counts))).unwrap();
    let _resolved = registry.resolve().unwrap();
    assert_eq!(registry.provider_ids().len(), 1);
    assert_eq!(counts[0].load(Ordering::SeqCst), 0, "factory calls");
    assert_eq!(counts[1].load(Ordering::SeqCst), 0, "I/O or model initialization");
    assert_eq!(counts[2].load(Ordering::SeqCst), 0, "executor startup");
}

#[test]
fn test_file_provider_rejects_classifier_without_same_object_support() {
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let group = manager.group(qubit_operation::GroupOptions { capacity: 2 }).unwrap();
    let services =
        MimeExecutionServices::new(manager, group, Arc::new(FixedMimeExecutionPort::new(1, 2)), 1024).unwrap();
    let detector_registry = MimeDetectorRegistry::builtin();
    let classifier_registry = MediaStreamClassifierRegistry::builtin();
    let error = detector_registry
        .create_async_backend_with_classifier(
            &qubit_spi::ProviderSelection::named("file").unwrap(),
            &MimeConfig::default(),
            &services,
            Some(
                classifier_registry
                    .create_async_backend(
                        &qubit_spi::ProviderSelection::named("ffprobe").unwrap(),
                        &MimeConfig::default(),
                        &services,
                    )
                    .unwrap()
                    .1,
            ),
        )
        .unwrap_err();

    assert!(matches!(
        error,
        MimeError::UnsupportedCapability {
            capability: MimeCapability::SameObjectRefinement
        }
    ));
}

#[derive(Debug)]
struct CountedProvider(Arc<[AtomicUsize; 3]>);

impl ServiceProvider<qubit_mime::MimeDetectorSpec> for CountedProvider {
    fn create_configured(
        &self,
        _config: &MimeConfig,
    ) -> Result<Arc<dyn qubit_mime::MimeDetector>, ProviderFailure<qubit_mime::MimeError>> {
        self.0[0].fetch_add(1, Ordering::SeqCst);
        self.0[1].fetch_add(1, Ordering::SeqCst);
        self.0[2].fetch_add(1, Ordering::SeqCst);
        Ok(Arc::new(crate::support::DirectBackendDetector::new()))
    }
}

impl ProviderMetadata for CountedProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        provider_descriptor!("counted-provider")
    }
}

#[test]
fn test_operation_provider_sync_only_is_rejected() {
    let provider = SyncPanicProvider;
    let registry = MimeDetectorRegistry::default();
    registry.register(provider).unwrap();
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let group = manager.group(qubit_operation::GroupOptions { capacity: 1 }).unwrap();
    let services =
        MimeExecutionServices::new(manager, group, Arc::new(FixedMimeExecutionPort::new(1, 1)), 1024).unwrap();
    let selection = qubit_spi::ProviderSelection::named("sync-panic").unwrap();
    let result = registry.create_async_backend(&selection, &MimeConfig::default(), &services);
    assert!(matches!(result, Err(qubit_mime::MimeError::AsyncUnsupported)));

    let detector = crate::support::DirectBackendDetector::new();
    let result = block_on(detector.guess_from_content_async(b"hello"));
    assert!(matches!(result, Err(qubit_mime::MimeError::AsyncUnsupported)));
}

#[derive(Debug)]
struct AsyncTestProvider(Arc<AtomicUsize>);

impl ProviderMetadata for AsyncTestProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor::new(ProviderId::new("sync-panic").unwrap())
    }
}

impl AsyncMimeDetectorProvider for AsyncTestProvider {
    fn create_async_backend(
        &self,
        _config: &MimeConfig,
        _services: &MimeExecutionServices,
    ) -> Result<Arc<dyn AsyncMimeDetectorBackend>, MimeError> {
        Ok(Arc::new(AsyncTestBackend(Arc::clone(&self.0))) as Arc<dyn AsyncMimeDetectorBackend>)
    }
}

#[derive(Debug)]
struct AsyncTestBackend(Arc<AtomicUsize>);

impl AsyncMimeDetectorBackend for AsyncTestBackend {
    fn content_requirement(&self) -> ContentRequirement {
        ContentRequirement::Prefix(4)
    }

    fn random_access_capability(&self) -> RandomAccessCapability {
        RandomAccessCapability::Unsupported
    }

    fn guess_content_in<'a>(
        &'a self,
        _scope: &'a qubit_operation::OperationScope,
        input: BorrowedMimeInput<'a>,
        _options: &'a MimeOperationOptions,
    ) -> MimeFuture<'a, MimeCandidates> {
        Box::pin(async move {
            let inspected = match input {
                BorrowedMimeInput::Bytes { content, .. } => content.len(),
                BorrowedMimeInput::Provider { .. } => {
                    panic!("prefix provider input must be read once")
                }
            };
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(MimeCandidates {
                candidates: vec![qubit_mime::MimeCandidate {
                    mime_type: "application/x-async".into(),
                    rank: 0,
                }],
                inspected_extent: InspectedExtent::Prefix(inspected),
                retained_prefix: vec![],
                staging_bytes: 0,
            })
        })
    }
}

#[test]
fn test_operation_provider_selected_backend_runs_async_without_sync_factory() {
    let registry = MimeDetectorRegistry::default();
    registry.register(SyncPanicProvider).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    registry
        .register_async_provider(AsyncTestProvider(Arc::clone(&calls)))
        .unwrap();

    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let group = manager.group(qubit_operation::GroupOptions { capacity: 4 }).unwrap();
    let services =
        MimeExecutionServices::new(manager, group, Arc::new(FixedMimeExecutionPort::new(1, 4)), 1024).unwrap();
    services.prepare().unwrap();
    let selection = qubit_spi::ProviderSelection::named("sync-panic").unwrap();
    let runtime = MimeRuntime::builtin()
        .unwrap()
        .with_execution_services(services.clone())
        .with_async_provider_selection(&registry, &selection)
        .unwrap();
    let mut operation = runtime
        .try_submit_detection(
            OwnedMimeInput::bytes(Arc::from(&b"data"[..]), InputExtent::Complete),
            MimeOperationOptions::default(),
        )
        .unwrap_or_else(|_| panic!("selected async provider should accept root operation"));
    let outcome = block_on(operation.wait_outcome()).unwrap();
    match outcome {
        qubit_operation::OperationOutcome::Succeeded(report) => {
            assert_eq!(report.mime_type.as_deref(), Some("application/x-async"));
            assert_eq!(report.backend_id, "sync-panic");
        }
        other => panic!("async backend operation failed: {other:?}"),
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let shutdown = services.shutdown();
    assert!(block_on(shutdown.wait_services_settled()).is_ok());
    assert!(shutdown.stop_services().is_ok());
}

#[test]
fn test_operation_provider_sync_child_keeps_repository_backend_after_async_selection() {
    let registry = MimeDetectorRegistry::default();
    registry.register(SyncPanicProvider).unwrap();
    registry
        .register_async_provider(AsyncTestProvider(Arc::new(AtomicUsize::new(0))))
        .unwrap();

    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let group = manager.group(qubit_operation::GroupOptions { capacity: 2 }).unwrap();
    let services = MimeExecutionServices::new(
        manager.clone(),
        group,
        Arc::new(FixedMimeExecutionPort::new(1, 2)),
        1024,
    )
    .unwrap();
    services.prepare().unwrap();
    let runtime = MimeRuntime::builtin()
        .unwrap()
        .with_execution_services(services.clone())
        .with_async_provider_selection(&registry, &qubit_spi::ProviderSelection::named("sync-panic").unwrap())
        .unwrap();

    let child_slot = Arc::new(Mutex::new(None));
    let body_slot = Arc::clone(&child_slot);
    let body_runtime = runtime.clone();
    let mut parent = manager
        .try_submit(Default::default(), move |scope| {
            let child = body_runtime.try_submit_detection_sync_in(
                &scope,
                OwnedMimeInput::bytes(Arc::from(&b"%PDF-1.7"[..]), InputExtent::Complete),
                MimeOperationOptions::default(),
            );
            match child {
                Ok(child) => {
                    *body_slot.lock().unwrap() = Some(child);
                    Ok::<_, MimeError>(true)
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
        qubit_operation::OperationOutcome::Succeeded(report) => {
            assert_eq!(report.mime_type.as_deref(), Some("application/pdf"));
            assert_eq!(report.backend_id, "repository");
        }
        other => panic!("sync child should use the runtime repository backend: {other:?}"),
    }

    let shutdown = services.shutdown();
    assert!(block_on(shutdown.wait_services_settled()).is_ok());
    assert!(shutdown.stop_services().is_ok());
}

#[test]
fn test_operation_provider_runtime_keeps_injected_repository() {
    let repository = MimeRepository::from_xml(
        r#"<mime-info xmlns="http://www.freedesktop.org/standards/shared-mime-info">
  <mime-type type="application/x-runtime-only">
    <magic priority="50"><match type="string" value="RUNTIME-ONLY" offset="0"/></magic>
  </mime-type>
</mime-info>"#,
    )
    .expect("custom runtime repository should parse");
    let runtime = MimeRuntime::new(MimeConfig::default(), Arc::new(repository));
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let group = manager.group(qubit_operation::GroupOptions { capacity: 4 }).unwrap();
    let services =
        MimeExecutionServices::new(manager, group, Arc::new(FixedMimeExecutionPort::new(1, 4)), 1024).unwrap();
    services.prepare().unwrap();
    let runtime = runtime.with_execution_services(services.clone());
    let mut operation = runtime
        .try_submit_detection(
            OwnedMimeInput::bytes(Arc::from(&b"RUNTIME-ONLY"[..]), InputExtent::Complete),
            MimeOperationOptions::default(),
        )
        .unwrap_or_else(|_| panic!("custom repository operation should be admitted"));
    let outcome = block_on(operation.wait_outcome()).unwrap();
    match outcome {
        qubit_operation::OperationOutcome::Succeeded(report) => {
            assert_eq!(report.mime_type.as_deref(), Some("application/x-runtime-only"));
        }
        other => panic!("runtime should use its injected repository: {other:?}"),
    }
    let shutdown = services.shutdown();
    assert!(block_on(shutdown.wait_services_settled()).is_ok());
    assert!(shutdown.stop_services().is_ok());
}

#[derive(Debug)]
struct RefinementProvider;

impl ProviderMetadata for RefinementProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor::new(ProviderId::new("async-refinement").unwrap())
    }
}

impl AsyncMimeDetectorProvider for RefinementProvider {
    fn create_async_backend(
        &self,
        config: &MimeConfig,
        _services: &MimeExecutionServices,
    ) -> Result<Arc<dyn AsyncMimeDetectorBackend>, MimeError> {
        Ok(Arc::new(RefinementBackend::new(config.clone(), None)))
    }

    fn create_async_backend_with_classifier(
        &self,
        config: &MimeConfig,
        _services: &MimeExecutionServices,
        classifier: Option<Arc<dyn AsyncMediaStreamClassifierBackend>>,
    ) -> Result<Arc<dyn AsyncMimeDetectorBackend>, MimeError> {
        Ok(Arc::new(RefinementBackend::new(config.clone(), classifier)))
    }
}

#[derive(Debug)]
struct RefinementBackend {
    core: MimeDetectorCore,
}

impl RefinementBackend {
    fn new(config: MimeConfig, classifier: Option<Arc<dyn AsyncMediaStreamClassifierBackend>>) -> Self {
        let mut core = MimeDetectorCore::new(config);
        core.set_media_stream_classifier(Some(Arc::new(PanicSyncClassifier)));
        core.set_async_media_stream_classifier(classifier);
        Self { core }
    }
}

impl AsyncMimeDetectorBackend for RefinementBackend {
    fn content_requirement(&self) -> ContentRequirement {
        ContentRequirement::Prefix(4)
    }

    fn random_access_capability(&self) -> RandomAccessCapability {
        RandomAccessCapability::Unsupported
    }

    fn guess_content_in<'a>(
        &'a self,
        scope: &'a qubit_operation::OperationScope,
        input: BorrowedMimeInput<'a>,
        options: &'a MimeOperationOptions,
    ) -> MimeFuture<'a, MimeCandidates> {
        Box::pin(async move {
            let filename = match input {
                BorrowedMimeInput::Bytes { filename, .. } => filename,
                BorrowedMimeInput::Provider { path, .. } => path.file_name(),
            };
            let from_filename = filename
                .filter(|name| name.ends_with(".webm"))
                .map(|_| vec!["video/webm".to_owned()])
                .unwrap_or_default();
            let selected = self
                .core
                .select_result_in(scope, &from_filename, &["video/webm".to_owned()], input, options)
                .await?;
            Ok(MimeCandidates {
                candidates: selected
                    .into_iter()
                    .map(|mime_type| MimeCandidate { mime_type, rank: 0 })
                    .collect(),
                inspected_extent: InspectedExtent::Prefix(4),
                retained_prefix: Vec::new(),
                staging_bytes: 0,
            })
        })
    }
}

#[derive(Debug)]
struct PanicSyncClassifier;

impl MediaStreamClassifier for PanicSyncClassifier {
    fn classify_file(&self, _file: &std::path::Path) -> Result<MediaStreamType, MimeError> {
        panic!("async managed refinement must not call sync classifier")
    }

    fn classify_reader(&self, _reader: &mut dyn std::io::Read) -> Result<MediaStreamType, MimeError> {
        panic!("async managed refinement must not call sync classifier")
    }
}

#[test]
fn test_operation_selected_async_classifier_refines_managed_runtime_without_sync_call() {
    let mut raw_config = qubit_config::Config::new();
    raw_config
        .set(qubit_mime::CONFIG_MIME_ENABLE_PRECISE_DETECTION, true)
        .unwrap();
    raw_config
        .set(qubit_mime::CONFIG_MIME_PRECISE_DETECTION_PATTERNS, "webm")
        .unwrap();
    raw_config
        .set(
            qubit_mime::CONFIG_MIME_AMBIGUOUS_MIME_MAPPING,
            "webm:video/webm,audio/webm",
        )
        .unwrap();
    let config = MimeConfig::from_config(&raw_config).unwrap();

    let detector_registry = MimeDetectorRegistry::default();
    detector_registry.register(SyncPanicProvider).unwrap();
    detector_registry.register_async_provider(RefinementProvider).unwrap();
    let classifier_calls = Arc::new(AtomicUsize::new(0));
    let classifier_registry = MediaStreamClassifierRegistry::default();
    classifier_registry
        .register_async_provider(AsyncClassifierProvider(Arc::clone(&classifier_calls)))
        .unwrap();

    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let group = manager.group(qubit_operation::GroupOptions { capacity: 8 }).unwrap();
    let services =
        MimeExecutionServices::new(manager, group, Arc::new(FixedMimeExecutionPort::new(1, 8)), 1024).unwrap();
    services.prepare().unwrap();

    let runtime =
        MimeRuntime::new(config, Arc::new(MimeRepository::bundled().clone())).with_execution_services(services.clone());
    let detector_selection = qubit_spi::ProviderSelection::named("async-refinement").unwrap();
    let classifier_selection = qubit_spi::ProviderSelection::named("async-classifier").unwrap();
    let selected = runtime
        .clone()
        .with_async_provider_selection_and_classifier(
            &detector_registry,
            &detector_selection,
            &classifier_registry,
            &classifier_selection,
        )
        .unwrap_or_else(|_| panic!("selected providers should build async backend"));

    let mut preferred = selected
        .try_submit_detection(
            OwnedMimeInput::bytes(Arc::from(&b"webm"[..]), InputExtent::Complete).with_filename("sample.webm"),
            MimeOperationOptions {
                policy: MimeDetectionPolicy::PreferFilename,
                ..MimeOperationOptions::default()
            },
        )
        .unwrap_or_else(|_| panic!("preferred runtime should admit operation"));
    assert!(matches!(
        block_on(preferred.wait_outcome()).unwrap(),
        qubit_operation::OperationOutcome::Succeeded(ref report)
            if report.mime_type.as_deref() == Some("video/webm")
    ));
    assert_eq!(classifier_calls.load(Ordering::SeqCst), 0);

    let mut refined = selected
        .try_submit_detection(
            OwnedMimeInput::bytes(Arc::from(&b"webm"[..]), InputExtent::Complete).with_filename("sample.webm"),
            MimeOperationOptions::default(),
        )
        .unwrap_or_else(|_| panic!("refining runtime should admit operation"));
    assert!(matches!(
        block_on(refined.wait_outcome()).unwrap(),
        qubit_operation::OperationOutcome::Succeeded(ref report)
            if report.mime_type.as_deref() == Some("audio/webm")
    ));
    assert_eq!(classifier_calls.load(Ordering::SeqCst), 1);

    let no_classifier = runtime
        .with_async_provider_selection(&detector_registry, &detector_selection)
        .unwrap_or_else(|_| panic!("runtime without classifier should admit operation"));
    let mut unchanged = no_classifier
        .try_submit_detection(
            OwnedMimeInput::bytes(Arc::from(&b"webm"[..]), InputExtent::Complete).with_filename("sample.webm"),
            MimeOperationOptions::default(),
        )
        .unwrap_or_else(|_| panic!("runtime without classifier should admit operation"));
    assert!(matches!(
        block_on(unchanged.wait_outcome()).unwrap(),
        qubit_operation::OperationOutcome::Succeeded(ref report)
            if report.mime_type.as_deref() == Some("video/webm")
    ));
    assert_eq!(classifier_calls.load(Ordering::SeqCst), 1);

    let shutdown = services.shutdown();
    assert!(block_on(shutdown.wait_services_settled()).is_ok());
    assert!(shutdown.stop_services().is_ok());
}

#[derive(Debug)]
struct SyncPanicProvider;

impl ServiceProvider<qubit_mime::MimeDetectorSpec> for SyncPanicProvider {
    fn create_configured(&self, _config: &MimeConfig) -> Result<Arc<dyn MimeDetector>, ProviderFailure<MimeError>> {
        panic!("sync-only async selection called sync factory")
    }
}

impl ProviderMetadata for SyncPanicProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor::new(ProviderId::new("sync-panic").unwrap())
    }
}

#[derive(Debug)]
struct AsyncClassifierProvider(Arc<AtomicUsize>);

impl ProviderMetadata for AsyncClassifierProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor::new(ProviderId::new("async-classifier").unwrap())
    }
}

impl AsyncMediaStreamClassifierProvider for AsyncClassifierProvider {
    fn create_async_backend(
        &self,
        _config: &MimeConfig,
        _services: &MimeExecutionServices,
    ) -> Result<Arc<dyn AsyncMediaStreamClassifierBackend>, MimeError> {
        Ok(Arc::new(AsyncTestClassifier(Arc::clone(&self.0))))
    }
}

#[derive(Debug)]
struct AsyncTestClassifier(Arc<AtomicUsize>);

impl AsyncMediaStreamClassifierBackend for AsyncTestClassifier {
    fn classify_in<'a>(
        &'a self,
        _scope: &'a qubit_operation::OperationScope,
        _input: BorrowedMimeInput<'a>,
        _options: &'a MimeOperationOptions,
    ) -> MimeFuture<'a, MediaStreamType> {
        Box::pin(async move {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(MediaStreamType::AudioOnly)
        })
    }
}

#[test]
fn test_operation_classifier_async_factory_is_selected_and_invoked() {
    let calls = Arc::new(AtomicUsize::new(0));
    let registry = MediaStreamClassifierRegistry::default();
    registry
        .register_async_provider(AsyncClassifierProvider(Arc::clone(&calls)))
        .unwrap();
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let group = manager.group(qubit_operation::GroupOptions { capacity: 2 }).unwrap();
    let services = MimeExecutionServices::new(
        manager,
        group.clone(),
        Arc::new(FixedMimeExecutionPort::new(1, 2)),
        1024,
    )
    .unwrap();
    let selection = qubit_spi::ProviderSelection::named("async-classifier").unwrap();
    let (_, backend) = registry
        .create_async_backend(&selection, &MimeConfig::default(), &services)
        .unwrap();
    let mut operation = group
        .as_async()
        .try_submit(Default::default(), move |scope| {
            let backend = Arc::clone(&backend);
            async move {
                backend
                    .classify_in(
                        &scope,
                        BorrowedMimeInput::Bytes {
                            content: b"sample",
                            extent: InputExtent::Complete,
                            filename: None,
                        },
                        &MimeOperationOptions::default(),
                    )
                    .await
            }
        })
        .unwrap();
    assert!(matches!(
        block_on(operation.wait_outcome()).unwrap(),
        qubit_operation::OperationOutcome::Succeeded(MediaStreamType::AudioOnly)
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}
