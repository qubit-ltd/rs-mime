use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Context, Poll, Wake, Waker},
};

use qubit_mime::MediaStreamClassifierRegistry;
use qubit_mime::detector::MimeRuntime;
use qubit_mime::{
    AsyncMediaStreamClassifierBackend, AsyncMediaStreamClassifierProvider,
    AsyncMimeDetectorBackend, AsyncMimeDetectorProvider, BorrowedMimeInput, ContentRequirement,
    FixedMimeExecutionPort, InputExtent, InspectedExtent, MediaStreamClassifier, MediaStreamType,
    MimeCandidate, MimeCandidates, MimeConfig, MimeDetectionPolicy, MimeDetector,
    MimeDetectorBackend, MimeDetectorCore, MimeDetectorRegistry, MimeError, MimeExecutionServices,
    MimeFuture, MimeOperationOptions, MimeRepository, OwnedMimeInput, RandomAccessCapability,
};
use qubit_spi::error::ProviderFailure;
use qubit_spi::{
    ProviderDescriptor, ProviderId, ProviderMetadata, ServiceProvider, provider_descriptor,
};

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
fn operation_provider_registration_is_pure() {
    let registry = MimeDetectorRegistry::default();
    let counts = Arc::new([
        AtomicUsize::new(0),
        AtomicUsize::new(0),
        AtomicUsize::new(0),
    ]);
    registry
        .register(CountedProvider(Arc::clone(&counts)))
        .unwrap();
    let _resolved = registry.resolve().unwrap();
    assert_eq!(registry.provider_ids().len(), 1);
    assert_eq!(counts[0].load(Ordering::SeqCst), 0, "factory calls");
    assert_eq!(
        counts[1].load(Ordering::SeqCst),
        0,
        "I/O or model initialization"
    );
    assert_eq!(counts[2].load(Ordering::SeqCst), 0, "executor startup");
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
fn operation_provider_sync_only_is_rejected() {
    let provider = SyncPanicProvider;
    let registry = MimeDetectorRegistry::default();
    registry.register(provider).unwrap();
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let group = manager
        .group(qubit_operation::GroupOptions { capacity: 1 })
        .unwrap();
    let services = MimeExecutionServices::new(
        manager,
        group,
        Arc::new(FixedMimeExecutionPort::new(1, 1)),
        1024,
    )
    .unwrap();
    let selection = qubit_spi::ProviderSelection::named("sync-panic").unwrap();
    let result = registry.create_async_backend(&selection, &MimeConfig::default(), &services);
    assert!(matches!(
        result,
        Err(qubit_mime::MimeError::AsyncUnsupported)
    ));

    let detector = crate::support::DirectBackendDetector::new();
    let result = block_on(detector.guess_from_content_async(b"hello"));
    assert!(matches!(
        result,
        Err(qubit_mime::MimeError::AsyncUnsupported)
    ));
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
            })
        })
    }
}

#[test]
fn operation_provider_selected_backend_runs_async_without_sync_factory() {
    let registry = MimeDetectorRegistry::default();
    registry.register(SyncPanicProvider).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    registry
        .register_async_provider(AsyncTestProvider(Arc::clone(&calls)))
        .unwrap();

    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let group = manager
        .group(qubit_operation::GroupOptions { capacity: 4 })
        .unwrap();
    let services = MimeExecutionServices::new(
        manager,
        group,
        Arc::new(FixedMimeExecutionPort::new(1, 4)),
        1024,
    )
    .unwrap();
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
fn operation_provider_runtime_keeps_injected_repository() {
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
    let services = MimeExecutionServices::new(
        manager,
        group,
        Arc::new(FixedMimeExecutionPort::new(1, 4)),
        1024,
    )
    .unwrap();
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
    fn new(
        config: MimeConfig,
        classifier: Option<Arc<dyn AsyncMediaStreamClassifierBackend>>,
    ) -> Self {
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
                .select_result_in(
                    scope,
                    &from_filename,
                    &["video/webm".to_owned()],
                    input,
                    options,
                )
                .await?;
            Ok(MimeCandidates {
                candidates: selected
                    .into_iter()
                    .map(|mime_type| MimeCandidate { mime_type, rank: 0 })
                    .collect(),
                inspected_extent: InspectedExtent::Prefix(4),
                retained_prefix: Vec::new(),
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

    fn classify_reader(
        &self,
        _reader: &mut dyn std::io::Read,
    ) -> Result<MediaStreamType, MimeError> {
        panic!("async managed refinement must not call sync classifier")
    }
}

#[test]
fn operation_selected_async_classifier_refines_managed_runtime_without_sync_call() {
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
    detector_registry
        .register_async_provider(RefinementProvider)
        .unwrap();
    let classifier_calls = Arc::new(AtomicUsize::new(0));
    let classifier_registry = MediaStreamClassifierRegistry::default();
    classifier_registry
        .register_async_provider(AsyncClassifierProvider(Arc::clone(&classifier_calls)))
        .unwrap();

    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let group = manager
        .group(qubit_operation::GroupOptions { capacity: 8 })
        .unwrap();
    let services = MimeExecutionServices::new(
        manager,
        group,
        Arc::new(FixedMimeExecutionPort::new(1, 8)),
        1024,
    )
    .unwrap();
    services.prepare().unwrap();

    let runtime = MimeRuntime::new(config, Arc::new(MimeRepository::bundled().clone()))
        .with_execution_services(services.clone());
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
            OwnedMimeInput::bytes(Arc::from(&b"webm"[..]), InputExtent::Complete)
                .with_filename("sample.webm"),
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
            OwnedMimeInput::bytes(Arc::from(&b"webm"[..]), InputExtent::Complete)
                .with_filename("sample.webm"),
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
            OwnedMimeInput::bytes(Arc::from(&b"webm"[..]), InputExtent::Complete)
                .with_filename("sample.webm"),
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
    fn create_configured(
        &self,
        _config: &MimeConfig,
    ) -> Result<Arc<dyn MimeDetector>, ProviderFailure<MimeError>> {
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
fn operation_classifier_async_factory_is_selected_and_invoked() {
    let calls = Arc::new(AtomicUsize::new(0));
    let registry = MediaStreamClassifierRegistry::default();
    registry
        .register_async_provider(AsyncClassifierProvider(Arc::clone(&calls)))
        .unwrap();
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let group = manager
        .group(qubit_operation::GroupOptions { capacity: 2 })
        .unwrap();
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
