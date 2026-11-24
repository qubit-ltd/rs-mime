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

use qubit_mime::AsyncMediaStreamClassifierBackend;
use qubit_mime::AsyncMediaStreamClassifierProvider;
use qubit_mime::AsyncMimeDetectorBackend;
use qubit_mime::AsyncMimeDetectorProvider;
use qubit_mime::BorrowedMimeInput;
use qubit_mime::ContentRequirement;
use qubit_mime::InputExtent;
use qubit_mime::MediaStreamClassifierRegistry;
use qubit_mime::MediaStreamType;
use qubit_mime::MimeCandidates;
use qubit_mime::MimeConfig;
use qubit_mime::MimeDetectorRegistry;
use qubit_mime::MimeError;
use qubit_mime::MimeExecutionServices;
use qubit_mime::MimeFuture;
use qubit_mime::MimeOperationOptions;
use qubit_mime::MimeRepository;
use qubit_mime::OwnedMimeInput;
use qubit_mime::RandomAccessCapability;
use qubit_mime::RepositoryMimeDetector;
use qubit_mime::detector::MimeRuntime;
use qubit_spi::ProviderDescriptor;
use qubit_spi::ProviderId;
use qubit_spi::ProviderMetadata;

#[derive(Debug)]
struct CountingFileSystem {
    properties: qubit_fs::spi::ProviderProperties,
    opens: Arc<AtomicUsize>,
    transferred: Option<Arc<AtomicUsize>>,
}

impl CountingFileSystem {
    fn new(opens: Arc<AtomicUsize>) -> Self {
        Self::with_semantics(opens, qubit_fs::path::PathSemantics::Hierarchical)
    }

    fn with_semantics(opens: Arc<AtomicUsize>, semantics: qubit_fs::path::PathSemantics) -> Self {
        use qubit_fs::metadata::FileSystemCapabilities;
        use qubit_fs::metadata::FileSystemCapability;
        use qubit_fs::metadata::FileSystemId;
        use qubit_fs::metadata::FileSystemInfo;
        use qubit_fs::metadata::FileSystemLimits;
        use qubit_fs::metadata::SymlinkPolicy;
        use qubit_fs::path::PathConstraints;
        use qubit_fs::spi::ProviderOperation;
        use qubit_fs::spi::ProviderOperations;
        use qubit_fs::spi::ProviderProperties;
        Self {
            properties: ProviderProperties::new(
                FileSystemInfo::new(
                    FileSystemId::new("mime-runtime-counting").unwrap(),
                    "mime-runtime-counting",
                    semantics,
                ),
                ProviderOperations::new().with(ProviderOperation::OpenReader),
                FileSystemCapabilities::new().with_guaranteed(FileSystemCapability::Read),
                FileSystemLimits::unknown(),
                PathConstraints::either(),
                SymlinkPolicy::Reject,
            )
            .unwrap(),
            opens,
            transferred: None,
        }
    }

    fn with_transfer_counter(mut self, transferred: Arc<AtomicUsize>) -> Self {
        self.transferred = Some(transferred);
        self
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
        let transferred = self.transferred.clone();
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
                    transferred,
                }),
            ))
        })
    }
}

struct PdfInput {
    bytes: Vec<u8>,
    position: usize,
    transferred: Option<Arc<AtomicUsize>>,
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
        if let Some(transferred) = &self.transferred {
            transferred.fetch_add(read, Ordering::SeqCst);
        }
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

#[derive(Debug)]
struct DeadlineClockState {
    ticks: u64,
    registrations: Vec<(u64, bool, Waker)>,
}

#[derive(Debug)]
struct DeadlineClock {
    domain: qubit_operation::ClockDomainId,
    state: Arc<std::sync::Mutex<DeadlineClockState>>,
}

impl DeadlineClock {
    fn new() -> Self {
        Self {
            domain: qubit_operation::ClockDomainId::new_unique(),
            state: Arc::new(std::sync::Mutex::new(DeadlineClockState {
                ticks: 0,
                registrations: Vec::new(),
            })),
        }
    }

    fn advance_to(&self, ticks: u64) {
        let wakers = {
            let mut state = self.state.lock().unwrap();
            state.ticks = ticks;
            state
                .registrations
                .iter()
                .filter(|(deadline, active, _)| *active && *deadline <= ticks)
                .map(|(_, _, waker)| waker.clone())
                .collect::<Vec<_>>()
        };
        for waker in wakers {
            waker.wake();
        }
    }

    fn active_registrations(&self) -> usize {
        self.state
            .lock()
            .unwrap()
            .registrations
            .iter()
            .filter(|(_, active, _)| *active)
            .count()
    }

    fn registration_count(&self) -> usize {
        self.state.lock().unwrap().registrations.len()
    }
}

impl qubit_operation::ClockSource for DeadlineClock {
    fn now(&self) -> qubit_operation::MonotonicTime {
        qubit_operation::MonotonicTime::new(self.domain, self.state.lock().unwrap().ticks)
    }

    fn register_deadline(
        &self,
        at: qubit_operation::MonotonicTime,
        wake: Waker,
    ) -> Box<dyn qubit_operation::TimerRegistration> {
        let index = {
            let mut state = self.state.lock().unwrap();
            let index = state.registrations.len();
            state.registrations.push((at.ticks(), true, wake));
            index
        };
        Box::new(DeadlineClockRegistration {
            state: Arc::clone(&self.state),
            index,
        })
    }
}

struct DeadlineClockRegistration {
    state: Arc<std::sync::Mutex<DeadlineClockState>>,
    index: usize,
}

impl qubit_operation::TimerRegistration for DeadlineClockRegistration {
    fn cancel(self: Box<Self>) {
        self.state.lock().unwrap().registrations[self.index].1 = false;
    }
}

#[derive(Debug)]
struct DeadlineYieldRepositoryProvider {
    repository: Arc<MimeRepository>,
    clock: Arc<DeadlineClock>,
    pending_observations: Arc<AtomicUsize>,
}

impl qubit_spi::ProviderMetadata for DeadlineYieldRepositoryProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor::new(ProviderId::new("deadline-yield-repository").unwrap())
    }
}

impl AsyncMimeDetectorProvider for DeadlineYieldRepositoryProvider {
    fn create_async_backend(
        &self,
        _config: &MimeConfig,
        _services: &MimeExecutionServices,
    ) -> Result<Arc<dyn AsyncMimeDetectorBackend>, MimeError> {
        Ok(Arc::new(DeadlineYieldRepositoryBackend {
            repository: Arc::clone(&self.repository),
            clock: Arc::clone(&self.clock),
            pending_observations: Arc::clone(&self.pending_observations),
        }))
    }
}

#[derive(Debug)]
struct DeadlineYieldRepositoryBackend {
    repository: Arc<MimeRepository>,
    clock: Arc<DeadlineClock>,
    pending_observations: Arc<AtomicUsize>,
}

impl AsyncMimeDetectorBackend for DeadlineYieldRepositoryBackend {
    fn content_requirement(&self) -> ContentRequirement {
        ContentRequirement::Prefix(self.repository.max_test_bytes())
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
        let repository = Arc::clone(&self.repository);
        let clock = Arc::clone(&self.clock);
        let pending_observations = Arc::clone(&self.pending_observations);
        Box::pin(async move {
            let detector = RepositoryMimeDetector::with_repository(&repository);
            let mut matching = Box::pin(detector.guess_content_in(scope, input, options));
            std::future::poll_fn(|cx| {
                let result = matching.as_mut().poll(cx);
                if result.is_pending() && pending_observations.fetch_add(1, Ordering::SeqCst) == 0 {
                    assert!(
                        clock.active_registrations() > 0,
                        "managed detection must register its deadline before polling the repository backend"
                    );
                    clock.advance_to(10);
                }
                result
            })
            .await
        })
    }
}

struct NeverWakeFileSystem {
    properties: qubit_fs::spi::ProviderProperties,
    reads: Arc<AtomicUsize>,
    reader_drops: Arc<AtomicUsize>,
    control: Arc<NeverWakeControl>,
}

impl NeverWakeFileSystem {
    fn new(reads: Arc<AtomicUsize>, reader_drops: Arc<AtomicUsize>, control: Arc<NeverWakeControl>) -> Self {
        let properties = CountingFileSystem::new(Arc::new(AtomicUsize::new(0))).properties;
        Self {
            properties,
            reads,
            reader_drops,
            control,
        }
    }
}

impl qubit_fs::spi::AsyncFileSystemSpi for NeverWakeFileSystem {
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
        let reads = Arc::clone(&self.reads);
        let reader_drops = Arc::clone(&self.reader_drops);
        let control = Arc::clone(&self.control);
        let info = qubit_fs::metadata::OpenedFileInfo::new(
            qubit_fs::metadata::FileSystemId::new("mime-runtime-counting").unwrap(),
            request.path().clone(),
        );
        Box::pin(async move {
            Ok(qubit_fs::spi::OpenedAsyncReader::new(
                info,
                Box::new(NeverWakeInput {
                    reads,
                    reader_drops,
                    control,
                }),
            ))
        })
    }
}

#[derive(Default)]
struct NeverWakeControl {
    state: std::sync::Mutex<(Option<Waker>, bool)>,
}

impl NeverWakeControl {
    fn release(&self) {
        let waker = {
            let mut state = self.state.lock().unwrap();
            state.1 = true;
            state.0.clone()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }
}

struct NeverWakeInput {
    reads: Arc<AtomicUsize>,
    reader_drops: Arc<AtomicUsize>,
    control: Arc<NeverWakeControl>,
}

impl Drop for NeverWakeInput {
    fn drop(&mut self) {
        self.reader_drops.fetch_add(1, Ordering::SeqCst);
    }
}

impl qubit_io::AsyncInput for NeverWakeInput {
    type Item = u8;

    unsafe fn poll_read_unchecked(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        _output: &mut [u8],
        _index: usize,
        _count: usize,
    ) -> Poll<std::io::Result<usize>> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        let mut state = self.control.state.lock().unwrap();
        if state.1 {
            Poll::Ready(Ok(0))
        } else {
            state.0 = Some(cx.waker().clone());
            Poll::Pending
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
fn test_managed_deadline_wakes_never_waking_provider_read_and_releases_reader() {
    use std::time::Duration;
    use std::time::Instant;

    use qubit_fs::AsyncFileSystem;
    use qubit_fs::Path;
    use qubit_operation::ClockSource;
    use qubit_operation::ContextOptions;
    use qubit_operation::Deadline;
    use qubit_operation::MonotonicTime;
    use qubit_operation::OperationOptions;
    use qubit_operation::OperationOutcome;
    use qubit_operation::StopReason;

    let repository = Arc::new(
        MimeRepository::from_xml(
            r#"<mime-info xmlns="http://www.freedesktop.org/standards/shared-mime-info">
  <mime-type type="application/x-never">
    <magic priority="80"><match type="string" value="NEVER" offset="0"/></magic>
  </mime-type>
</mime-info>"#,
        )
        .unwrap(),
    );
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let port = Arc::new(TestExecution::default());
    let services = services(&manager, 4, Arc::clone(&port));
    services.prepare().unwrap();
    let runtime = MimeRuntime::new(MimeConfig::default(), repository).with_execution_services(services.clone());
    let clock = Arc::new(DeadlineClock::new());
    let clock_source: Arc<dyn ClockSource> = clock.clone();
    let control = Arc::new(NeverWakeControl::default());
    let reads = Arc::new(AtomicUsize::new(0));
    let reader_drops = Arc::new(AtomicUsize::new(0));
    let file_system = Arc::new(
        AsyncFileSystem::from_spi(NeverWakeFileSystem::new(
            Arc::clone(&reads),
            Arc::clone(&reader_drops),
            Arc::clone(&control),
        ))
        .unwrap(),
    );
    let mut operation = match runtime.try_submit_detection(
        OwnedMimeInput::Provider {
            file_system,
            path: Path::parse("/pending.data").unwrap(),
        },
        MimeOperationOptions {
            operation: OperationOptions {
                context: ContextOptions {
                    clock: Some(clock_source),
                    deadline: Some(Deadline::at(MonotonicTime::new(clock.domain, 10))),
                    ..Default::default()
                },
                ..Default::default()
            },
            ..Default::default()
        },
    ) {
        Ok(operation) => operation,
        Err(failure) => panic!("provider operation should be accepted: {:?}", failure.reason),
    };

    let startup = Instant::now();
    while reads.load(Ordering::SeqCst) == 0 && startup.elapsed() < Duration::from_secs(2) {
        std::thread::yield_now();
    }
    assert!(reads.load(Ordering::SeqCst) > 0, "provider read should be pending");
    let registered = clock.active_registrations() > 0;
    clock.advance_to(10);

    let wait_for_outcome = |operation: &mut qubit_mime::AsyncMimeOperation| {
        let started = Instant::now();
        loop {
            if let Some(outcome) = operation.try_take_outcome().unwrap() {
                return Some(outcome);
            }
            if started.elapsed() >= Duration::from_millis(500) {
                return None;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    };
    let mut outcome = wait_for_outcome(&mut operation);
    if outcome.is_none() {
        control.release();
        outcome = wait_for_outcome(&mut operation);
    }

    assert!(registered, "the accepted operation must register its deadline");
    assert!(
        matches!(outcome, Some(OperationOutcome::Stopped(ref report)) if report.reason == StopReason::Timeout),
        "deadline should win while the provider read is pending: {outcome:?}"
    );
    let _settlement = block_on(operation.wait_settlement());
    assert_eq!(
        1,
        reader_drops.load(Ordering::SeqCst),
        "the pending reader must be released"
    );
    assert_eq!(
        0,
        clock.active_registrations(),
        "the completed deadline timer must be cancelled"
    );

    let shutdown = services.shutdown();
    let _ = block_on(shutdown.wait_services_settled()).unwrap();
    let _ = shutdown.stop_services().unwrap();
    let _ = block_on(shutdown.wait_group_shutdown()).unwrap();
}

#[test]
fn test_managed_deadline_stops_cooperative_repository_matching() {
    use qubit_operation::ClockSource;
    use qubit_operation::ContextOptions;
    use qubit_operation::Deadline;
    use qubit_operation::MonotonicTime;
    use qubit_operation::OperationOptions;
    use qubit_operation::OperationOutcome;
    use qubit_operation::StopReason;

    let magic_rules = (0..8192)
        .map(|index| {
            format!(
                "<mime-type type=\"application/x-deadline-{index}\"><magic priority=\"80\"><match type=\"string\" value=\"NEVER{index}\" offset=\"0\"/></magic></mime-type>"
            )
        })
        .collect::<String>();
    let repository = Arc::new(
        MimeRepository::from_xml(&format!(
            "<mime-info xmlns=\"http://www.freedesktop.org/standards/shared-mime-info\">{magic_rules}</mime-info>"
        ))
        .unwrap(),
    );
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let services = services(&manager, 4, Arc::new(TestExecution::default()));
    services.prepare().unwrap();
    let clock = Arc::new(DeadlineClock::new());
    let pending_observations = Arc::new(AtomicUsize::new(0));
    let registry = MimeDetectorRegistry::default();
    registry
        .register_async_provider(DeadlineYieldRepositoryProvider {
            repository: Arc::clone(&repository),
            clock: Arc::clone(&clock),
            pending_observations: Arc::clone(&pending_observations),
        })
        .unwrap();
    let runtime = MimeRuntime::new(MimeConfig::default(), repository)
        .with_execution_services(services.clone())
        .with_async_provider_selection(
            &registry,
            &qubit_spi::ProviderSelection::named("deadline-yield-repository").unwrap(),
        )
        .unwrap();
    let clock_source: Arc<dyn ClockSource> = clock.clone();
    let mut operation = runtime
        .try_submit_detection(
            OwnedMimeInput::bytes(Arc::from(b"X".as_slice()), InputExtent::Complete),
            MimeOperationOptions {
                operation: OperationOptions {
                    context: ContextOptions {
                        clock: Some(clock_source),
                        deadline: Some(Deadline::at(MonotonicTime::new(clock.domain, 10))),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .unwrap_or_else(|failure| panic!("matcher operation should be accepted: {:?}", failure.reason));

    let outcome = block_on(operation.wait_outcome()).unwrap();
    assert_eq!(
        1,
        pending_observations.load(Ordering::SeqCst),
        "backend should yield once before timeout"
    );
    assert!(
        clock.registration_count() > 0,
        "the accepted operation must register its deadline"
    );
    assert!(matches!(
        outcome,
        OperationOutcome::Stopped(ref report) if report.reason == StopReason::Timeout
    ));
    let _settlement = block_on(operation.wait_settlement());
    assert_eq!(0, clock.active_registrations(), "deadline timer should be cancelled");

    let shutdown = services.shutdown();
    let _ = block_on(shutdown.wait_services_settled()).unwrap();
    let _ = shutdown.stop_services().unwrap();
    let _ = block_on(shutdown.wait_group_shutdown()).unwrap();
}

#[test]
fn test_user_cancel_wins_when_racing_a_managed_deadline() {
    use std::time::Duration;
    use std::time::Instant;

    use qubit_fs::AsyncFileSystem;
    use qubit_fs::Path;
    use qubit_operation::ClockSource;
    use qubit_operation::ContextOptions;
    use qubit_operation::Deadline;
    use qubit_operation::MonotonicTime;
    use qubit_operation::OperationOptions;
    use qubit_operation::OperationOutcome;
    use qubit_operation::StopReason;

    let repository = Arc::new(
        MimeRepository::from_xml(
            r#"<mime-info xmlns="http://www.freedesktop.org/standards/shared-mime-info">
  <mime-type type="application/x-never">
    <magic priority="80"><match type="string" value="NEVER" offset="0"/></magic>
  </mime-type>
</mime-info>"#,
        )
        .unwrap(),
    );
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let services = services(&manager, 4, Arc::new(TestExecution::default()));
    services.prepare().unwrap();
    let runtime = MimeRuntime::new(MimeConfig::default(), repository).with_execution_services(services.clone());
    let clock = Arc::new(DeadlineClock::new());
    let clock_source: Arc<dyn ClockSource> = clock.clone();
    let control = Arc::new(NeverWakeControl::default());
    let reads = Arc::new(AtomicUsize::new(0));
    let reader_drops = Arc::new(AtomicUsize::new(0));
    let file_system = Arc::new(
        AsyncFileSystem::from_spi(NeverWakeFileSystem::new(
            Arc::clone(&reads),
            Arc::clone(&reader_drops),
            Arc::clone(&control),
        ))
        .unwrap(),
    );
    let mut operation = runtime
        .try_submit_detection(
            OwnedMimeInput::Provider {
                file_system,
                path: Path::parse("/pending.data").unwrap(),
            },
            MimeOperationOptions {
                operation: OperationOptions {
                    context: ContextOptions {
                        clock: Some(clock_source),
                        deadline: Some(Deadline::at(MonotonicTime::new(clock.domain, 10))),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .unwrap_or_else(|failure| panic!("provider operation should be accepted: {:?}", failure.reason));

    let startup = Instant::now();
    while reads.load(Ordering::SeqCst) == 0 && startup.elapsed() < Duration::from_secs(2) {
        std::thread::yield_now();
    }
    assert!(reads.load(Ordering::SeqCst) > 0, "provider read should be pending");
    assert!(
        clock.active_registrations() > 0,
        "operation should register its deadline"
    );
    assert_eq!(
        qubit_operation::StopRequestStatus::Accepted,
        operation.request_stop(StopReason::UserCancelled),
    );
    clock.advance_to(10);

    let deadline = Instant::now() + Duration::from_secs(2);
    let outcome = loop {
        if let Some(outcome) = operation.try_take_outcome().unwrap() {
            break outcome;
        }
        if Instant::now() >= deadline {
            control.release();
            panic!("explicit cancellation did not wake the provider read");
        }
        std::thread::yield_now();
    };
    assert!(matches!(
        outcome,
        OperationOutcome::Stopped(ref report) if report.reason == StopReason::UserCancelled
    ));
    let _settlement = block_on(operation.wait_settlement());
    assert_eq!(
        1,
        reader_drops.load(Ordering::SeqCst),
        "cancellation should release the reader"
    );
    assert_eq!(0, clock.active_registrations(), "stop should cancel its deadline timer");

    let shutdown = services.shutdown();
    let _ = block_on(shutdown.wait_services_settled()).unwrap();
    let _ = shutdown.stop_services().unwrap();
    let _ = block_on(shutdown.wait_group_shutdown()).unwrap();
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

#[derive(Debug)]
struct PrefixClassifierProvider(Arc<AtomicUsize>);

impl ProviderMetadata for PrefixClassifierProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor::new(ProviderId::new("prefix-counting-classifier").unwrap())
    }
}

impl AsyncMediaStreamClassifierProvider for PrefixClassifierProvider {
    fn create_async_backend(
        &self,
        _config: &MimeConfig,
        _services: &MimeExecutionServices,
    ) -> Result<Arc<dyn AsyncMediaStreamClassifierBackend>, MimeError> {
        Ok(Arc::new(PrefixClassifier(Arc::clone(&self.0))))
    }
}

#[derive(Debug)]
struct PrefixClassifier(Arc<AtomicUsize>);

impl AsyncMediaStreamClassifierBackend for PrefixClassifier {
    fn classify_in<'a>(
        &'a self,
        _scope: &'a qubit_operation::OperationScope,
        input: BorrowedMimeInput<'a>,
        _options: &'a MimeOperationOptions,
    ) -> MimeFuture<'a, MediaStreamType> {
        Box::pin(async move {
            self.0.fetch_add(1, Ordering::SeqCst);
            match input {
                BorrowedMimeInput::Bytes {
                    extent: InputExtent::Prefix,
                    ..
                } => Err(MimeError::CompleteContentRequired),
                _ => Ok(MediaStreamType::AudioOnly),
            }
        })
    }
}

#[test]
fn test_managed_prefix_skips_complete_media_classifier() {
    let mut raw_config = qubit_config::Config::new();
    raw_config
        .set(qubit_mime::CONFIG_MIME_ENABLE_PRECISE_DETECTION, true)
        .unwrap();
    raw_config
        .set(qubit_mime::CONFIG_MIME_PRECISE_DETECTION_PATTERNS, "ogg")
        .unwrap();
    raw_config
        .set(
            qubit_mime::CONFIG_MIME_AMBIGUOUS_MIME_MAPPING,
            "ogg:video/ogg,audio/x-vorbis+ogg",
        )
        .unwrap();
    let config = MimeConfig::from_config(&raw_config).unwrap();
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let group = manager.group(qubit_operation::GroupOptions { capacity: 4 }).unwrap();
    let services = MimeExecutionServices::new(
        manager,
        group,
        Arc::new(qubit_mime::FixedMimeExecutionPort::new(1, 4)),
        1024 * 1024,
    )
    .unwrap();
    services.prepare().unwrap();

    let calls = Arc::new(AtomicUsize::new(0));
    let detector_registry = MimeDetectorRegistry::builtin();
    let classifier_registry = MediaStreamClassifierRegistry::builtin();
    classifier_registry
        .register_async_provider(PrefixClassifierProvider(Arc::clone(&calls)))
        .unwrap();
    let runtime = MimeRuntime::new(config, Arc::new(qubit_mime::MimeRepository::bundled().clone()))
        .with_execution_services(services.clone())
        .with_async_provider_selection_and_classifier(
            &detector_registry,
            &qubit_spi::ProviderSelection::named("repository").unwrap(),
            &classifier_registry,
            &qubit_spi::ProviderSelection::named("prefix-counting-classifier").unwrap(),
        )
        .unwrap();
    let media: Arc<[u8]> = Arc::from(include_bytes!("../fixtures/real_files/test.ogg").as_slice());

    let mut prefix = runtime
        .try_submit_detection(
            OwnedMimeInput::bytes(Arc::clone(&media), InputExtent::Prefix).with_filename("clip.ogg"),
            MimeOperationOptions::default(),
        )
        .unwrap_or_else(|_| panic!("prepared Prefix operation should be admitted"));
    let prefix_outcome = block_on(prefix.wait_outcome()).unwrap();
    assert!(
        matches!(
            &prefix_outcome,
            qubit_operation::OperationOutcome::Succeeded(report)
                if report.mime_type.as_deref() == Some("audio/x-vorbis+ogg")
                    && report.budget.staging_bytes == 0
        ),
        "unexpected Prefix outcome: {prefix_outcome:?}"
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "Prefix must skip complete-content classification"
    );

    let mut complete = runtime
        .try_submit_detection(
            OwnedMimeInput::bytes(Arc::clone(&media), InputExtent::Complete).with_filename("clip.ogg"),
            MimeOperationOptions::default(),
        )
        .unwrap_or_else(|_| panic!("prepared Complete operation should be admitted"));
    assert!(matches!(
        block_on(complete.wait_outcome()).unwrap(),
        qubit_operation::OperationOutcome::Succeeded(ref report)
            if report.mime_type.as_deref() == Some("audio/x-vorbis+ogg")
    ));
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "Complete input must invoke the classifier"
    );

    let shutdown = services.shutdown();
    assert!(block_on(shutdown.wait_services_settled()).is_ok());
    assert!(shutdown.stop_services().is_ok());
}

#[test]
fn test_explicit_builtin_repository_selection_short_circuits_provider_reads() {
    use qubit_fs::AsyncFileSystem;
    use qubit_fs::Path;

    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let services = services(&manager, 4, Arc::new(TestExecution::default()));
    services.prepare().unwrap();
    let registry = MimeDetectorRegistry::builtin();
    let classifier_registry = MediaStreamClassifierRegistry::builtin();
    let classifier_calls = Arc::new(AtomicUsize::new(0));
    classifier_registry
        .register_async_provider(PrefixClassifierProvider(Arc::clone(&classifier_calls)))
        .unwrap();

    let selections = [
        qubit_spi::ProviderSelection::named("repository").unwrap(),
        qubit_spi::ProviderSelection::named("repository-mime-detector").unwrap(),
        qubit_spi::ProviderSelection::chain_allowing_missing(["missing", "repository"]).unwrap(),
    ];
    let mut total_opens = 0;
    for selection in selections {
        let opens = Arc::new(AtomicUsize::new(0));
        let file_system = Arc::new(AsyncFileSystem::from_spi(CountingFileSystem::new(Arc::clone(&opens))).unwrap());
        let runtime = MimeRuntime::builtin()
            .unwrap()
            .with_execution_services(services.clone())
            .with_async_provider_selection_and_classifier(
                &registry,
                &selection,
                &classifier_registry,
                &qubit_spi::ProviderSelection::named("prefix-counting-classifier").unwrap(),
            )
            .unwrap();
        let mut operation = runtime
            .try_submit_detection(
                OwnedMimeInput::Provider {
                    file_system,
                    path: Path::parse("/report.pdf").unwrap(),
                },
                MimeOperationOptions {
                    policy: qubit_mime::MimeDetectionPolicy::PreferFilename,
                    ..Default::default()
                },
            )
            .unwrap_or_else(|failure| panic!("filename result should be admitted: {:?}", failure.reason));
        let report = match block_on(operation.wait_outcome()).unwrap() {
            qubit_operation::OperationOutcome::Succeeded(report) => report,
            other => panic!("explicit built-in selection should succeed: {other:?}"),
        };
        assert_eq!(Some("application/pdf"), report.mime_type.as_deref());
        assert_eq!(qubit_mime::InspectedExtent::Prefix(0), report.inspected_extent);
        assert_eq!(0, report.budget.bytes_read);
        assert_eq!(0, report.budget.staging_bytes);
        total_opens += opens.load(Ordering::SeqCst);
    }
    assert_eq!(0, total_opens, "selected repository should avoid opening the Provider");
    assert_eq!(
        0,
        classifier_calls.load(Ordering::SeqCst),
        "filename short circuit should skip classifier"
    );

    let shutdown = services.shutdown();
    let _ = block_on(shutdown.wait_services_settled()).unwrap();
    let _ = shutdown.stop_services().unwrap();
    let _ = block_on(shutdown.wait_group_shutdown()).unwrap();
}

#[test]
fn test_managed_provider_uses_native_basename_for_filename_short_circuit() {
    use qubit_fs::AsyncFileSystem;
    use qubit_fs::Path;

    let repository = MimeRepository::from_xml(
        r#"<mime-info xmlns="http://www.freedesktop.org/standards/shared-mime-info">
  <mime-type type="application/x-native-basename">
    <glob pattern="left\right.mimecase" weight="80"/>
  </mime-type>
  <mime-type type="application/x-string-basename">
    <glob pattern="right.mimecase" weight="50"/>
  </mime-type>
</mime-info>"#,
    )
    .expect("custom filename repository should parse");
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let services = services(&manager, 4, Arc::new(TestExecution::default()));
    services.prepare().unwrap();
    let opens = Arc::new(AtomicUsize::new(0));
    let file_system = Arc::new(AsyncFileSystem::from_spi(CountingFileSystem::new(Arc::clone(&opens))).unwrap());
    let runtime =
        MimeRuntime::new(MimeConfig::default(), Arc::new(repository)).with_execution_services(services.clone());

    let mut operation = runtime
        .try_submit_detection(
            OwnedMimeInput::Provider {
                file_system,
                path: Path::parse(r"/left\right.mimecase").unwrap(),
            },
            MimeOperationOptions {
                policy: qubit_mime::MimeDetectionPolicy::PreferFilename,
                ..Default::default()
            },
        )
        .unwrap_or_else(|failure| panic!("filename result should be admitted: {:?}", failure.reason));
    let report = match block_on(operation.wait_outcome()).unwrap() {
        qubit_operation::OperationOutcome::Succeeded(report) => report,
        other => panic!("native basename short circuit should succeed: {other:?}"),
    };

    assert_eq!(Some("application/x-native-basename"), report.mime_type.as_deref());
    assert_eq!(qubit_mime::InspectedExtent::Prefix(0), report.inspected_extent);
    assert_eq!(
        0,
        opens.load(Ordering::SeqCst),
        "filename short circuit must not read the provider path"
    );

    let mut operation = runtime
        .try_submit_detection(
            OwnedMimeInput::bytes(Arc::from(b"".as_slice()), InputExtent::Complete)
                .with_filename(r"left\right.mimecase"),
            MimeOperationOptions {
                policy: qubit_mime::MimeDetectionPolicy::PreferFilename,
                ..Default::default()
            },
        )
        .unwrap_or_else(|failure| panic!("string filename result should be admitted: {:?}", failure.reason));
    let report = match block_on(operation.wait_outcome()).unwrap() {
        qubit_operation::OperationOutcome::Succeeded(report) => report,
        other => panic!("string filename short circuit should succeed: {other:?}"),
    };
    assert_eq!(Some("application/x-string-basename"), report.mime_type.as_deref());
    assert_eq!(qubit_mime::InspectedExtent::Prefix(0), report.inspected_extent);
    assert_eq!(
        0,
        opens.load(Ordering::SeqCst),
        "string filename matching must not open the unrelated provider"
    );

    let shutdown = services.shutdown();
    let _ = block_on(shutdown.wait_services_settled()).unwrap();
    let _ = shutdown.stop_services().unwrap();
    let _ = block_on(shutdown.wait_group_shutdown()).unwrap();
}

#[test]
fn test_managed_provider_candidate_fallback_preserves_object_key_basename() {
    use qubit_fs::AsyncFileSystem;
    use qubit_fs::Path;

    let repository = MimeRepository::from_xml(
        r#"<mime-info xmlns="http://www.freedesktop.org/standards/shared-mime-info">
  <mime-type type="application/x-native-basename">
    <glob pattern="left\right.mimecase" weight="80"/>
  </mime-type>
  <mime-type type="application/x-string-basename">
    <glob pattern="right.mimecase" weight="50"/>
  </mime-type>
  <mime-type type="application/x-unmatched-content">
    <magic priority="80"><match type="string" value="NEVER" offset="0"/></magic>
  </mime-type>
</mime-info>"#,
    )
    .expect("custom filename repository should parse");
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let services = services(&manager, 4, Arc::new(TestExecution::default()));
    services.prepare().unwrap();
    let opens = Arc::new(AtomicUsize::new(0));
    let file_system = Arc::new(
        AsyncFileSystem::from_spi(CountingFileSystem::with_semantics(
            Arc::clone(&opens),
            qubit_fs::path::PathSemantics::ObjectKey,
        ))
        .unwrap(),
    );
    let runtime =
        MimeRuntime::new(MimeConfig::default(), Arc::new(repository)).with_execution_services(services.clone());

    for (policy, expected) in [
        (
            qubit_mime::MimeDetectionPolicy::ContentFirst,
            Some("application/x-native-basename"),
        ),
        (
            qubit_mime::MimeDetectionPolicy::Freedesktop,
            Some("application/x-native-basename"),
        ),
        (qubit_mime::MimeDetectionPolicy::VerifyContent, None),
    ] {
        let mut operation = runtime
            .try_submit_detection(
                OwnedMimeInput::Provider {
                    file_system: Arc::clone(&file_system),
                    path: Path::parse_literal(r"/left\right.mimecase").unwrap(),
                },
                MimeOperationOptions {
                    policy,
                    ..Default::default()
                },
            )
            .unwrap_or_else(|failure| panic!("provider request should be admitted: {:?}", failure.reason));
        let report = match block_on(operation.wait_outcome()).unwrap() {
            qubit_operation::OperationOutcome::Succeeded(report) => report,
            other => panic!("provider content detection should succeed: {other:?}"),
        };

        assert_eq!(expected, report.mime_type.as_deref());
        assert_eq!(4, report.budget.bytes_read);
    }

    assert_eq!(
        3,
        opens.load(Ordering::SeqCst),
        "non-short-circuit policies must read the provider"
    );
    let shutdown = services.shutdown();
    let _ = block_on(shutdown.wait_services_settled()).unwrap();
    let _ = shutdown.stop_services().unwrap();
    let _ = block_on(shutdown.wait_group_shutdown()).unwrap();
}

#[derive(Debug)]
struct RepositoryIdCustomProvider(Arc<AtomicUsize>);

impl ProviderMetadata for RepositoryIdCustomProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor::new(ProviderId::new("repository").unwrap())
    }
}

impl AsyncMimeDetectorProvider for RepositoryIdCustomProvider {
    fn create_async_backend(
        &self,
        _config: &MimeConfig,
        _services: &MimeExecutionServices,
    ) -> Result<Arc<dyn AsyncMimeDetectorBackend>, MimeError> {
        Ok(Arc::new(RepositoryIdCustomBackend(Arc::clone(&self.0))))
    }
}

#[derive(Debug)]
struct RepositoryIdCustomBackend(Arc<AtomicUsize>);

impl AsyncMimeDetectorBackend for RepositoryIdCustomBackend {
    fn content_requirement(&self) -> ContentRequirement {
        ContentRequirement::Complete
    }

    fn random_access_capability(&self) -> RandomAccessCapability {
        RandomAccessCapability::Unsupported
    }

    fn guess_content_in<'a>(
        &'a self,
        _scope: &'a qubit_operation::OperationScope,
        _input: BorrowedMimeInput<'a>,
        _options: &'a MimeOperationOptions,
    ) -> MimeFuture<'a, MimeCandidates> {
        Box::pin(async move {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(MimeCandidates {
                candidates: Vec::new(),
                inspected_extent: qubit_mime::InspectedExtent::Complete,
                retained_prefix: Vec::new(),
                staging_bytes: 0,
            })
        })
    }
}

#[derive(Debug)]
struct ManagedParentBudgetProvider(Arc<AtomicUsize>);

impl ProviderMetadata for ManagedParentBudgetProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor::new(ProviderId::new("managed-parent-budget").unwrap())
    }
}

impl AsyncMimeDetectorProvider for ManagedParentBudgetProvider {
    fn create_async_backend(
        &self,
        _config: &MimeConfig,
        _services: &MimeExecutionServices,
    ) -> Result<Arc<dyn AsyncMimeDetectorBackend>, MimeError> {
        Ok(Arc::new(ManagedParentBudgetBackend(Arc::clone(&self.0))))
    }
}

#[derive(Debug)]
struct ManagedParentBudgetBackend(Arc<AtomicUsize>);

impl AsyncMimeDetectorBackend for ManagedParentBudgetBackend {
    fn content_requirement(&self) -> ContentRequirement {
        ContentRequirement::Prefix(4)
    }

    fn random_access_capability(&self) -> RandomAccessCapability {
        RandomAccessCapability::Unsupported
    }

    fn uses_managed_provider_reads(&self) -> bool {
        true
    }

    fn guess_content_in<'a>(
        &'a self,
        _scope: &'a qubit_operation::OperationScope,
        input: BorrowedMimeInput<'a>,
        _options: &'a MimeOperationOptions,
    ) -> MimeFuture<'a, MimeCandidates> {
        Box::pin(async move {
            self.0.fetch_add(1, Ordering::SeqCst);
            let BorrowedMimeInput::ManagedProvider { session, .. } = input else {
                panic!("managed parent-budget backend must receive a metered session")
            };
            let mut bytes = [0; 4];
            let count = session.read(&mut bytes).await?;
            assert_eq!(&bytes[..count], &b"%PDF"[..count]);
            Ok(MimeCandidates {
                candidates: vec![qubit_mime::MimeCandidate {
                    mime_type: "application/x-managed-parent-budget".into(),
                    rank: 0,
                }],
                inspected_extent: qubit_mime::InspectedExtent::Prefix(count),
                retained_prefix: Vec::new(),
                staging_bytes: 0,
            })
        })
    }
}

#[test]
fn test_custom_provider_using_repository_id_does_not_get_repository_short_circuit() {
    use qubit_fs::AsyncFileSystem;
    use qubit_fs::Path;

    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let services = services(&manager, 2, Arc::new(TestExecution::default()));
    services.prepare().unwrap();
    let provider_calls = Arc::new(AtomicUsize::new(0));
    let registry = MimeDetectorRegistry::default();
    registry
        .register_async_provider(RepositoryIdCustomProvider(Arc::clone(&provider_calls)))
        .unwrap();
    let runtime = MimeRuntime::builtin()
        .unwrap()
        .with_execution_services(services.clone())
        .with_async_provider_selection(&registry, &qubit_spi::ProviderSelection::named("repository").unwrap())
        .unwrap();
    let file_system =
        Arc::new(AsyncFileSystem::from_spi(CountingFileSystem::new(Arc::new(AtomicUsize::new(0)))).unwrap());
    let mut operation = runtime
        .try_submit_detection(
            OwnedMimeInput::Provider {
                file_system,
                path: Path::parse("/report.pdf").unwrap(),
            },
            MimeOperationOptions {
                policy: qubit_mime::MimeDetectionPolicy::PreferFilename,
                ..Default::default()
            },
        )
        .unwrap_or_else(|failure| panic!("custom backend should be admitted: {:?}", failure.reason));
    let report = match block_on(operation.wait_outcome()).unwrap() {
        qubit_operation::OperationOutcome::Succeeded(report) => report,
        other => panic!("custom backend should succeed: {other:?}"),
    };
    assert_eq!(
        None,
        report.mime_type.as_deref(),
        "custom result must not use the bundled filename match"
    );
    assert_eq!(1, provider_calls.load(Ordering::SeqCst), "custom backend must execute");

    let shutdown = services.shutdown();
    let _ = block_on(shutdown.wait_services_settled()).unwrap();
    let _ = shutdown.stop_services().unwrap();
    let _ = block_on(shutdown.wait_group_shutdown()).unwrap();
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
    let _ = shutdown.stop_services().unwrap();
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
    let _ = block_on(shutdown.wait_services_settled()).unwrap();
    let _ = shutdown.stop_services().unwrap();
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
    let _ = block_on(shutdown.wait_services_settled()).unwrap();
    let _ = shutdown.stop_services().unwrap();
    let _report = block_on(shutdown.wait_group_shutdown()).unwrap();
}

#[test]
fn test_operation_runtime_drain_accepts_external_parent_child_until_stop_freezes() {
    use std::sync::mpsc;

    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let group = manager.group(qubit_operation::GroupOptions { capacity: 4 }).unwrap();
    let port = Arc::new(TestExecution::default());
    let services = qubit_mime::MimeExecutionServices::new(manager, group.clone(), port.clone(), 1024).unwrap();
    services.prepare().unwrap();
    let runtime = MimeRuntime::builtin()
        .unwrap()
        .with_execution_services(services.clone());
    let body_runtime = runtime.clone();
    let (parent_started_tx, parent_started_rx) = mpsc::channel();
    let (submit_first_tx, submit_first_rx) = mpsc::channel();
    let (first_accepted_tx, first_accepted_rx) = mpsc::channel();
    let (submit_second_tx, submit_second_rx) = mpsc::channel();
    let (second_rejected_tx, second_rejected_rx) = mpsc::channel();

    let mut external_parent = block_on(group.try_submit_async(Default::default(), move |scope| async move {
        parent_started_tx.send(()).unwrap();
        submit_first_rx.recv().unwrap();
        let mut first_child = body_runtime
            .try_submit_detection_in(
                &scope,
                OwnedMimeInput::bytes(Arc::from(b"%PDF-1.7".as_slice()), InputExtent::Complete),
                MimeOperationOptions::default(),
            )
            .unwrap_or_else(|_| panic!("valid external parent child should be accepted during Drain"));
        first_accepted_tx.send(()).unwrap();

        submit_second_rx.recv().unwrap();
        let second_was_rejected = body_runtime
            .try_submit_detection_in(
                &scope,
                OwnedMimeInput::bytes(Arc::from(b"%PDF-1.7".as_slice()), InputExtent::Complete),
                MimeOperationOptions::default(),
            )
            .is_err();
        second_rejected_tx.send(second_was_rejected).unwrap();

        assert!(matches!(
            first_child.wait_outcome().await.unwrap(),
            qubit_operation::OperationOutcome::Succeeded(_)
        ));
        Ok::<_, ()>(())
    }))
    .unwrap();
    parent_started_rx.recv().unwrap();

    let shutdown = services.shutdown();
    assert!(block_on(shutdown.wait_services_settled()).unwrap().operations_settled);

    submit_first_tx.send(()).unwrap();
    first_accepted_rx.recv().unwrap();
    assert!(!shutdown.snapshot().operations_settled);
    assert!(matches!(
        shutdown.stop_services(),
        Err(MimeError::OperationShutdownPending)
    ));
    assert_eq!(0, port.shutdowns.load(Ordering::SeqCst));

    submit_second_tx.send(()).unwrap();
    assert!(second_rejected_rx.recv().unwrap());
    assert!(matches!(
        block_on(external_parent.wait_outcome()).unwrap(),
        qubit_operation::OperationOutcome::Succeeded(())
    ));
    block_on(external_parent.wait_settlement());
    drop(external_parent);

    assert!(block_on(shutdown.wait_services_settled()).unwrap().operations_settled);
    assert!(shutdown.stop_services().unwrap().services_stopped);
    assert_eq!(1, port.shutdowns.load(Ordering::SeqCst));
    let report = block_on(shutdown.wait_group_shutdown()).unwrap();
    assert_eq!(0, report.snapshot.active_operations);
    assert_eq!(0, report.snapshot.queued_operations);
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
    let _ = block_on(shutdown.wait_services_settled()).unwrap();
    let _ = shutdown.stop_services().unwrap();
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
    let _ = block_on(shutdown.wait_services_settled()).unwrap();
    let _ = shutdown.stop_services().unwrap();
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
    let _ = block_on(shutdown.wait_services_settled()).unwrap();
    let _ = shutdown.stop_services().unwrap();
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
        qubit_operation::OperationOutcome::Succeeded(report) => {
            assert_eq!(report.budget.owned_bytes, 8)
        }
        other => panic!("sync child should succeed: {other:?}"),
    }
    let shutdown = services.shutdown();
    let _ = block_on(shutdown.wait_services_settled()).unwrap();
    let _ = shutdown.stop_services().unwrap();
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
                (
                    InputExtent::Complete,
                    qubit_mime::MimeDetectionPolicy::VerifyContent,
                    "extent.pdf",
                ),
                (
                    InputExtent::Prefix,
                    qubit_mime::MimeDetectionPolicy::VerifyContent,
                    "extent.pdf",
                ),
                (
                    InputExtent::Complete,
                    qubit_mime::MimeDetectionPolicy::PreferFilename,
                    "unique-name.pdf",
                ),
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
        let expected_async_extent = if index == 2 {
            qubit_mime::InspectedExtent::Prefix(0)
        } else {
            qubit_mime::InspectedExtent::Prefix(max_test_bytes)
        };
        assert_eq!(
            expected_async_extent, async_report.inspected_extent,
            "async repository PreferFilename agrees with the synchronous policy"
        );
    }

    let shutdown = services.shutdown();
    let _ = block_on(shutdown.wait_services_settled()).unwrap();
    let _ = shutdown.stop_services().unwrap();
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
    let _ = block_on(shutdown.wait_services_settled()).unwrap();
    let _ = shutdown.stop_services().unwrap();
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
            let services_closed = matches!(
                failure.reason,
                qubit_mime::MimeSubmitRejection::ExecutionServicesRequired
            );
            let same_input = match failure.input {
                OwnedMimeInput::Bytes { content, .. } => Arc::ptr_eq(&bytes, &content),
                OwnedMimeInput::Provider { .. } => false,
            };
            Ok::<_, ()>(services_closed && same_input)
        })
        .unwrap();
    entered_rx.recv().unwrap();
    let shutdown = services.shutdown();
    let _ = block_on(shutdown.wait_services_settled()).unwrap();
    let _ = shutdown.stop_services().unwrap();
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
    let _ = block_on(shutdown.wait_services_settled()).unwrap();
    let _ = shutdown.stop_services().unwrap();
    let _report = block_on(shutdown.wait_group_shutdown()).unwrap();
}

#[test]
fn test_repository_prefer_filename_short_circuits_owned_bytes_and_provider() {
    use qubit_fs::AsyncFileSystem;
    use qubit_fs::Path;

    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let services = services(&manager, 4, Arc::new(TestExecution::default()));
    services.prepare().unwrap();
    let runtime = MimeRuntime::builtin()
        .unwrap()
        .with_execution_services(services.clone());

    let mut bytes = runtime
        .try_submit_detection(
            OwnedMimeInput::bytes(Arc::from(b"not a PDF".as_slice()), InputExtent::Complete)
                .with_filename("document.pdf"),
            MimeOperationOptions {
                policy: qubit_mime::MimeDetectionPolicy::PreferFilename,
                ..Default::default()
            },
        )
        .unwrap_or_else(|failure| panic!("bytes operation should be admitted: {:?}", failure.reason));
    let bytes_report = match block_on(bytes.wait_outcome()).unwrap() {
        qubit_operation::OperationOutcome::Succeeded(report) => report,
        other => panic!("filename-selected bytes request should succeed: {other:?}"),
    };
    assert_eq!(bytes_report.mime_type.as_deref(), Some("application/pdf"));
    assert_eq!(bytes_report.inspected_extent, qubit_mime::InspectedExtent::Prefix(0));
    assert_eq!(bytes_report.budget.owned_bytes, b"not a PDF".len());
    assert_eq!(bytes_report.budget.bytes_read, 0);
    assert_eq!(bytes_report.budget.staging_bytes, 0);

    let mut non_ascii = runtime
        .try_submit_detection(
            OwnedMimeInput::bytes(Arc::from(b"not text".as_slice()), InputExtent::Complete)
                .with_filename("报告.txt"),
            MimeOperationOptions {
                policy: qubit_mime::MimeDetectionPolicy::PreferFilename,
                ..Default::default()
            },
        )
        .unwrap_or_else(|failure| panic!("non-ASCII filename operation should be admitted: {:?}", failure.reason));
    let non_ascii_report = match block_on(non_ascii.wait_outcome()).unwrap() {
        qubit_operation::OperationOutcome::Succeeded(report) => report,
        other => panic!("non-ASCII filename result should succeed: {other:?}"),
    };
    assert_eq!(non_ascii_report.mime_type.as_deref(), Some("text/plain"));
    assert_eq!(non_ascii_report.inspected_extent, qubit_mime::InspectedExtent::Prefix(0));
    assert_eq!(non_ascii_report.budget.bytes_read, 0);

    let opens = Arc::new(AtomicUsize::new(0));
    let fs = Arc::new(AsyncFileSystem::from_spi(CountingFileSystem::new(Arc::clone(&opens))).unwrap());
    let mut provider = runtime
        .try_submit_detection(
            OwnedMimeInput::Provider {
                file_system: fs,
                path: Path::parse("/document.pdf").unwrap(),
            },
            MimeOperationOptions {
                policy: qubit_mime::MimeDetectionPolicy::PreferFilename,
                ..Default::default()
            },
        )
        .unwrap_or_else(|failure| panic!("provider operation should be admitted: {:?}", failure.reason));
    let provider_report = match block_on(provider.wait_outcome()).unwrap() {
        qubit_operation::OperationOutcome::Succeeded(report) => report,
        other => panic!("filename-selected provider request should succeed: {other:?}"),
    };
    assert_eq!(provider_report.mime_type.as_deref(), Some("application/pdf"));
    assert_eq!(provider_report.inspected_extent, qubit_mime::InspectedExtent::Prefix(0));
    assert_eq!(provider_report.budget.owned_bytes, 0);
    assert_eq!(provider_report.budget.bytes_read, 0);
    assert_eq!(provider_report.budget.staging_bytes, 0);
    assert_eq!(opens.load(Ordering::SeqCst), 0, "provider must not be opened");

    let shutdown = services.shutdown();
    let _ = block_on(shutdown.wait_services_settled()).unwrap();
    let _ = shutdown.stop_services().unwrap();
    let _report = block_on(shutdown.wait_group_shutdown()).unwrap();
}

#[test]
fn test_filename_short_circuit_rechecks_services_after_shutdown() {
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let services = services(&manager, 1, Arc::new(TestExecution::default()));
    services.prepare().unwrap();
    let runtime = MimeRuntime::builtin()
        .unwrap()
        .with_execution_services(services.clone());
    let shutdown = services.shutdown();

    let failure = match runtime.try_submit_detection(
        OwnedMimeInput::bytes(Arc::from(b"ignored".as_slice()), InputExtent::Complete)
            .with_filename("document.pdf"),
        MimeOperationOptions {
            policy: qubit_mime::MimeDetectionPolicy::PreferFilename,
            ..Default::default()
        },
    ) {
        Ok(_) => panic!("closed services must reject filename short-circuit submissions"),
        Err(failure) => failure,
    };

    assert!(matches!(failure.reason, qubit_mime::MimeSubmitRejection::ExecutionServicesRequired));
    assert!(matches!(failure.input, OwnedMimeInput::Bytes { .. }));
    let _ = block_on(shutdown.wait_services_settled()).unwrap();
    let _ = shutdown.stop_services().unwrap();
    let _report = block_on(shutdown.wait_group_shutdown()).unwrap();
}

#[test]
fn test_filename_short_circuit_submission_races_service_shutdown() {
    use std::sync::Barrier;
    use std::thread;

    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let services = services(&manager, 1, Arc::new(TestExecution::default()));
    services.prepare().unwrap();
    let runtime = MimeRuntime::builtin()
        .unwrap()
        .with_execution_services(services.clone());
    let barrier = Arc::new(Barrier::new(3));

    let submit_runtime = runtime.clone();
    let submit_barrier = Arc::clone(&barrier);
    let submitter = thread::spawn(move || {
        submit_barrier.wait();
        submit_runtime.try_submit_detection(
            OwnedMimeInput::bytes(Arc::from(b"ignored".as_slice()), InputExtent::Complete)
                .with_filename("document.pdf"),
            MimeOperationOptions {
                policy: qubit_mime::MimeDetectionPolicy::PreferFilename,
                ..Default::default()
            },
        )
    });

    let closing_services = services.clone();
    let close_barrier = Arc::clone(&barrier);
    let closer = thread::spawn(move || {
        close_barrier.wait();
        closing_services.shutdown()
    });
    barrier.wait();

    let submission = submitter.join().expect("submission thread should complete");
    let shutdown = closer.join().expect("shutdown thread should complete");
    match submission {
        Ok(mut operation) => match block_on(operation.wait_outcome()).unwrap() {
            qubit_operation::OperationOutcome::Succeeded(report) => {
                assert_eq!(report.mime_type.as_deref(), Some("application/pdf"));
                assert_eq!(report.budget.bytes_read, 0);
            }
            other => panic!("admitted filename short circuit should succeed: {other:?}"),
        },
        Err(failure) => {
            assert!(matches!(
                failure.reason,
                qubit_mime::MimeSubmitRejection::ExecutionServicesRequired
            ));
            assert!(matches!(failure.input, OwnedMimeInput::Bytes { .. }));
        }
    }

    let _ = block_on(shutdown.wait_services_settled()).unwrap();
    let _ = shutdown.stop_services().unwrap();
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
    #[cfg(coverage)]
    services.poison_read_tree_registration_lock_for_coverage();
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
    assert_eq!(reports.len(), 1, "unexpected sibling outcomes: {outcomes:#?}");
    assert_eq!(read_failures, 1, "unexpected sibling outcomes: {outcomes:#?}");
    assert_eq!(reports[0].budget.bytes_read, 4);
    assert_eq!(reports[0].budget.owned_bytes, 4);
    assert_eq!(
        opens.load(Ordering::SeqCst),
        1,
        "the rejected cumulative read must not reach the provider"
    );

    let shutdown = services.shutdown();
    let _ = block_on(shutdown.wait_services_settled()).unwrap();
    let _ = shutdown.stop_services().unwrap();
    let _report = block_on(shutdown.wait_group_shutdown()).unwrap();
}

#[test]
fn test_sequential_mime_children_keep_external_parent_read_budget() {
    use qubit_fs::AsyncFileSystem;
    use qubit_fs::Path;
    use qubit_operation::ChildFailurePolicy;
    use qubit_operation::OperationOutcome;

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
    let parent_options = qubit_operation::OperationOptions {
        child_failure_policy: ChildFailurePolicy::ContinueSiblings,
        limits: qubit_operation::ScopeLimits {
            resources: 1,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut parent = manager
        .as_async()
        .try_submit(parent_options, move |scope| {
            let body_runtime = runtime.clone();
            async move {
                let new_input = || OwnedMimeInput::Provider {
                    file_system: Arc::new(
                        AsyncFileSystem::from_spi(CountingFileSystem::new(Arc::clone(&opens_body))).unwrap(),
                    ),
                    path: Path::parse("/document.pdf").unwrap(),
                };
                let options = MimeOperationOptions {
                    max_buffer_bytes: Some(4),
                    operation: qubit_operation::OperationOptions {
                        requirement: qubit_operation::ChildRequirement::Optional,
                        ..Default::default()
                    },
                    ..Default::default()
                };

                let mut first = body_runtime
                    .try_submit_detection_in(&scope, new_input(), options.clone())
                    .unwrap_or_else(|_| panic!("the first child should be admitted"));
                let first_succeeded = matches!(first.wait_outcome().await.unwrap(), OperationOutcome::Succeeded(_));
                let _first_settlement = first.wait_settlement().await;
                drop(first);

                let mut second = body_runtime
                    .try_submit_detection_in(&scope, new_input(), options)
                    .unwrap_or_else(|_| panic!("the second child should be admitted before it reads"));
                let second_rejected_by_budget = matches!(
                    second.wait_outcome().await.unwrap(),
                    OperationOutcome::Failed(qubit_mime::MimeError::BudgetExceeded { kind: "read", .. })
                );
                let _second_settlement = second.wait_settlement().await;
                drop(second);

                Ok::<_, ()>((first_succeeded, second_rejected_by_budget))
            }
        })
        .unwrap();
    let (first_succeeded, second_rejected_by_budget) = match block_on(parent.wait_outcome()).unwrap() {
        OperationOutcome::Succeeded(result) => result,
        other => panic!("external parent should settle successfully: {other:?}"),
    };
    block_on(parent.wait_settlement());

    assert!(first_succeeded);
    assert!(second_rejected_by_budget);
    assert_eq!(
        1,
        opens.load(Ordering::SeqCst),
        "the second child must be stopped by the shared budget before opening its provider"
    );

    let shutdown = services.shutdown();
    let _ = block_on(shutdown.wait_services_settled()).unwrap();
    let _ = shutdown.stop_services().unwrap();
    let _report = block_on(shutdown.wait_group_shutdown()).unwrap();
}

#[test]
fn test_managed_provider_sibling_children_share_external_parent_read_budget() {
    use qubit_fs::AsyncFileSystem;
    use qubit_fs::Path;
    use qubit_operation::ChildFailurePolicy;
    use qubit_operation::OperationOutcome;

    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let group = manager.group(qubit_operation::GroupOptions { capacity: 4 }).unwrap();
    let services =
        qubit_mime::MimeExecutionServices::new(manager.clone(), group, Arc::new(TestExecution::default()), 4).unwrap();
    services.prepare().unwrap();

    let backend_calls = Arc::new(AtomicUsize::new(0));
    let registry = MimeDetectorRegistry::default();
    registry
        .register_async_provider(ManagedParentBudgetProvider(Arc::clone(&backend_calls)))
        .unwrap();
    let runtime = MimeRuntime::builtin()
        .unwrap()
        .with_execution_services(services.clone())
        .with_async_provider_selection(
            &registry,
            &qubit_spi::ProviderSelection::named("managed-parent-budget").unwrap(),
        )
        .unwrap();

    let opens = Arc::new(AtomicUsize::new(0));
    let transferred = Arc::new(AtomicUsize::new(0));
    let parent_runtime = runtime.clone();
    let opens_body = Arc::clone(&opens);
    let transferred_body = Arc::clone(&transferred);
    let parent_options = qubit_operation::OperationOptions {
        child_failure_policy: ChildFailurePolicy::ContinueSiblings,
        limits: qubit_operation::ScopeLimits {
            child_operations: 2,
            resources: 1,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut parent = manager
        .as_async()
        .try_submit(parent_options, move |scope| async move {
            let child_options = MimeOperationOptions {
                max_buffer_bytes: Some(4),
                max_read_bytes: Some(4),
                operation: qubit_operation::OperationOptions {
                    requirement: qubit_operation::ChildRequirement::Optional,
                    ..Default::default()
                },
                ..Default::default()
            };
            let scope = Arc::new(scope);
            let barrier = Arc::new(std::sync::Barrier::new(3));
            let first_runtime = parent_runtime.clone();
            let first_scope = Arc::clone(&scope);
            let first_barrier = Arc::clone(&barrier);
            let first_file_system = Arc::new(
                AsyncFileSystem::from_spi(
                    CountingFileSystem::new(Arc::clone(&opens_body))
                        .with_transfer_counter(Arc::clone(&transferred_body)),
                )
                .unwrap(),
            );
            let first_options = child_options.clone();
            let first = std::thread::spawn(move || {
                first_barrier.wait();
                first_runtime.try_submit_detection_in(
                    &first_scope,
                    OwnedMimeInput::Provider {
                        file_system: first_file_system,
                        path: Path::parse("/document.pdf").unwrap(),
                    },
                    first_options,
                )
            });

            let second_runtime = parent_runtime.clone();
            let second_scope = Arc::clone(&scope);
            let second_barrier = Arc::clone(&barrier);
            let second_file_system = Arc::new(
                AsyncFileSystem::from_spi(
                    CountingFileSystem::new(Arc::clone(&opens_body))
                        .with_transfer_counter(Arc::clone(&transferred_body)),
                )
                .unwrap(),
            );
            let second = std::thread::spawn(move || {
                second_barrier.wait();
                second_runtime.try_submit_detection_in(
                    &second_scope,
                    OwnedMimeInput::Provider {
                        file_system: second_file_system,
                        path: Path::parse("/document.pdf").unwrap(),
                    },
                    child_options,
                )
            });
            barrier.wait();
            let mut first = first
                .join()
                .expect("first child submission thread should finish")
                .unwrap_or_else(|failure| panic!("first managed child should be admitted: {:?}", failure.reason));
            let mut second = second
                .join()
                .expect("second child submission thread should finish")
                .unwrap_or_else(|failure| panic!("second managed child should be admitted: {:?}", failure.reason));

            let first_outcome = first.wait_outcome().await.unwrap();
            let second_outcome = second.wait_outcome().await.unwrap();
            let _ = first.wait_settlement().await;
            let _ = second.wait_settlement().await;
            drop((first, second));

            let mut reported_bytes = 0;
            let mut successful_reports = 0;
            let mut budget_failures = 0;
            for outcome in [first_outcome, second_outcome] {
                match outcome {
                    OperationOutcome::Succeeded(report) => {
                        successful_reports += 1;
                        reported_bytes += report.budget.bytes_read;
                    }
                    OperationOutcome::Failed(qubit_mime::MimeError::BudgetExceeded { kind: "read", .. }) => {
                        budget_failures += 1;
                    }
                    other => panic!("unexpected managed sibling result: {other:?}"),
                }
            }
            Ok::<_, ()>((reported_bytes, successful_reports, budget_failures))
        })
        .unwrap();

    let (reported_bytes, successful_reports, budget_failures) = match block_on(parent.wait_outcome()).unwrap() {
        OperationOutcome::Succeeded(result) => result,
        other => panic!("external parent should settle after an optional child read failure: {other:?}"),
    };
    block_on(parent.wait_settlement());
    assert_eq!(1, successful_reports);
    assert_eq!(1, budget_failures);
    assert_eq!(
        2,
        backend_calls.load(Ordering::SeqCst),
        "both admitted children reach the extension backend"
    );
    assert_eq!(
        2,
        opens.load(Ordering::SeqCst),
        "each admitted child opens its selected provider once"
    );
    assert_eq!(
        4,
        transferred.load(Ordering::SeqCst),
        "the shared parent budget allows only four delivered bytes"
    );
    assert_eq!(
        4, reported_bytes,
        "the successful child report accounts for all delivered bytes"
    );
    assert_eq!(transferred.load(Ordering::SeqCst) as u64, reported_bytes);

    let shutdown = services.shutdown();
    let _ = block_on(shutdown.wait_services_settled()).unwrap();
    let _ = shutdown.stop_services().unwrap();
    let _ = block_on(shutdown.wait_group_shutdown()).unwrap();
}

#[test]
fn test_managed_provider_child_admission_rejection_returns_original_input() {
    use qubit_fs::AsyncFileSystem;
    use qubit_fs::Path;
    use qubit_operation::OperationOutcome;

    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let group = manager.group(qubit_operation::GroupOptions { capacity: 2 }).unwrap();
    let services =
        qubit_mime::MimeExecutionServices::new(manager.clone(), group, Arc::new(TestExecution::default()), 4).unwrap();
    services.prepare().unwrap();

    let backend_calls = Arc::new(AtomicUsize::new(0));
    let registry = MimeDetectorRegistry::default();
    registry
        .register_async_provider(ManagedParentBudgetProvider(Arc::clone(&backend_calls)))
        .unwrap();
    let runtime = MimeRuntime::builtin()
        .unwrap()
        .with_execution_services(services.clone())
        .with_async_provider_selection(
            &registry,
            &qubit_spi::ProviderSelection::named("managed-parent-budget").unwrap(),
        )
        .unwrap();

    let opens = Arc::new(AtomicUsize::new(0));
    let file_system = Arc::new(AsyncFileSystem::from_spi(CountingFileSystem::new(Arc::clone(&opens))).unwrap());
    let input_path = Path::parse("/document.pdf").unwrap();
    let parent_runtime = runtime.clone();
    let child_file_system = Arc::clone(&file_system);
    let child_path = input_path.clone();
    let parent_options = qubit_operation::OperationOptions {
        limits: qubit_operation::ScopeLimits {
            child_operations: 0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut parent = manager
        .as_async()
        .try_submit(parent_options, move |scope| async move {
            let failure = match parent_runtime.try_submit_detection_in(
                &scope,
                OwnedMimeInput::Provider {
                    file_system: Arc::clone(&child_file_system),
                    path: child_path.clone(),
                },
                MimeOperationOptions::default(),
            ) {
                Ok(_) => panic!("parent child-operation limit should reject managed input"),
                Err(failure) => failure,
            };
            let admission_rejected = matches!(failure.reason, qubit_mime::MimeSubmitRejection::Admission(_));
            let input_returned = match failure.input {
                OwnedMimeInput::Provider { file_system, path } => {
                    Arc::ptr_eq(&file_system, &child_file_system) && path == child_path
                }
                OwnedMimeInput::Bytes { .. } => false,
            };
            Ok::<_, ()>((admission_rejected, input_returned))
        })
        .unwrap();

    let (admission_rejected, input_returned) = match block_on(parent.wait_outcome()).unwrap() {
        OperationOutcome::Succeeded(result) => result,
        other => panic!("parent should complete after rejecting its child: {other:?}"),
    };
    block_on(parent.wait_settlement());
    assert!(admission_rejected);
    assert!(
        input_returned,
        "rejection must return the original provider filesystem and path"
    );
    assert_eq!(0, opens.load(Ordering::SeqCst), "rejected input must never be opened");
    assert_eq!(
        0,
        backend_calls.load(Ordering::SeqCst),
        "rejected input must never reach the backend"
    );

    let shutdown = services.shutdown();
    let _ = block_on(shutdown.wait_services_settled()).unwrap();
    let _ = shutdown.stop_services().unwrap();
    let _ = block_on(shutdown.wait_group_shutdown()).unwrap();
}

#[test]
fn test_external_parent_read_budget_binding_failures_return_input() {
    use qubit_fs::AsyncFileSystem;
    use qubit_fs::Path;

    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let port = Arc::new(TestExecution::default());
    let group = manager.group(qubit_operation::GroupOptions { capacity: 4 }).unwrap();
    let services = qubit_mime::MimeExecutionServices::new(manager.clone(), group, port, 16).unwrap();
    services.prepare().unwrap();

    let backend_calls = Arc::new(AtomicUsize::new(0));
    let registry = MimeDetectorRegistry::default();
    registry
        .register_async_provider(ManagedParentBudgetProvider(Arc::clone(&backend_calls)))
        .unwrap();
    let runtime = MimeRuntime::builtin()
        .unwrap()
        .with_execution_services(services.clone())
        .with_async_provider_selection(
            &registry,
            &qubit_spi::ProviderSelection::named("managed-parent-budget").unwrap(),
        )
        .unwrap();
    let opens = Arc::new(AtomicUsize::new(0));
    let file_system = Arc::new(AsyncFileSystem::from_spi(CountingFileSystem::new(Arc::clone(&opens))).unwrap());
    let path = Path::parse("/budget-probe.unmatched").unwrap();

    let body_runtime = runtime.clone();
    let body_file_system = Arc::clone(&file_system);
    let body_path = path.clone();
    let capacity_parent_options = qubit_operation::OperationOptions {
        limits: qubit_operation::ScopeLimits {
            resources: 1,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut capacity_parent = manager
        .as_async()
        .try_submit(capacity_parent_options, move |scope| async move {
            let _occupied = scope.manage(Arc::new(())).unwrap();
            let result = body_runtime.try_submit_detection_in(
                &scope,
                OwnedMimeInput::Provider {
                    file_system: Arc::clone(&body_file_system),
                    path: body_path.clone(),
                },
                MimeOperationOptions::default(),
            );
            let (capacity_rejected, input_returned) = match result {
                Err(failure) => {
                    let capacity_rejected = matches!(
                        failure.reason,
                        qubit_mime::MimeSubmitRejection::ReadBudgetBinding(
                            qubit_operation::ResourceManageErrorKind::CapacityExceeded
                        )
                    );
                    let input_returned = match failure.input {
                        OwnedMimeInput::Provider { file_system, path } => {
                            Arc::ptr_eq(&body_file_system, &file_system) && path == body_path
                        }
                        OwnedMimeInput::Bytes { .. } => false,
                    };
                    (capacity_rejected, input_returned)
                }
                Ok(_) => (false, false),
            };
            Ok::<_, ()>((capacity_rejected, input_returned))
        })
        .unwrap();
    let (capacity_rejected, capacity_input_returned) = match block_on(capacity_parent.wait_outcome()).unwrap() {
        qubit_operation::OperationOutcome::Succeeded(result) => result,
        _ => panic!("capacity test parent should complete"),
    };
    block_on(capacity_parent.wait_settlement());
    assert!(capacity_rejected);
    assert!(capacity_input_returned);

    let body_runtime = runtime.clone();
    let body_file_system = Arc::clone(&file_system);
    let body_path = path.clone();
    let mut ended_parent = manager
        .as_async()
        .try_submit(
            Default::default(),
            move |scope| async move { Ok::<_, ()>(Arc::new(scope)) },
        )
        .unwrap();
    let ended_scope = match block_on(ended_parent.wait_outcome()).unwrap() {
        qubit_operation::OperationOutcome::Succeeded(scope) => scope,
        _ => panic!("ended-scope test parent should complete"),
    };
    block_on(ended_parent.wait_settlement());

    let failure = match body_runtime.try_submit_detection_in(
        &ended_scope,
        OwnedMimeInput::Provider {
            file_system: Arc::clone(&body_file_system),
            path: body_path.clone(),
        },
        MimeOperationOptions::default(),
    ) {
        Ok(_) => panic!("an ended parent must not start a child with a fresh budget"),
        Err(failure) => failure,
    };
    assert!(matches!(
        failure.reason,
        qubit_mime::MimeSubmitRejection::ReadBudgetBinding(qubit_operation::ResourceManageErrorKind::ScopeEnded)
    ));
    match failure.input {
        OwnedMimeInput::Provider { file_system, path } => {
            assert!(Arc::ptr_eq(&body_file_system, &file_system));
            assert_eq!(body_path, path);
        }
        OwnedMimeInput::Bytes { .. } => panic!("the rejected input changed kind"),
    }

    let body_runtime = runtime.clone();
    let body_file_system = Arc::clone(&file_system);
    let body_path = path.clone();
    let stopped_rejection = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stopped_rejection_body = Arc::clone(&stopped_rejection);
    let mut stopped_parent = manager
        .as_async()
        .try_submit(Default::default(), move |scope| async move {
            assert_eq!(
                scope.request_stop(qubit_operation::StopReason::UserCancelled),
                qubit_operation::StopRequestStatus::Accepted
            );
            let result = body_runtime.try_submit_detection_in(
                &scope,
                OwnedMimeInput::Provider {
                    file_system: Arc::clone(&body_file_system),
                    path: body_path.clone(),
                },
                MimeOperationOptions::default(),
            );
            let rejected = match result {
                Err(failure) => {
                    let stopped = matches!(
                        failure.reason,
                        qubit_mime::MimeSubmitRejection::ReadBudgetBinding(
                            qubit_operation::ResourceManageErrorKind::Stopped
                        )
                    );
                    let returned = match failure.input {
                        OwnedMimeInput::Provider { file_system, path } => {
                            Arc::ptr_eq(&body_file_system, &file_system) && path == body_path
                        }
                        OwnedMimeInput::Bytes { .. } => false,
                    };
                    stopped && returned
                }
                Ok(_) => false,
            };
            stopped_rejection_body.store(rejected, Ordering::SeqCst);
            Ok::<_, ()>(())
        })
        .unwrap();
    let _stopped_outcome = block_on(stopped_parent.wait_outcome()).unwrap();
    block_on(stopped_parent.wait_settlement());
    assert!(stopped_rejection.load(Ordering::SeqCst));
    assert_eq!(
        0,
        opens.load(Ordering::SeqCst),
        "rejected provider input must not be opened"
    );
    assert_eq!(
        0,
        backend_calls.load(Ordering::SeqCst),
        "rejected provider input must not reach the backend"
    );

    let shutdown = services.shutdown();
    let _ = block_on(shutdown.wait_services_settled()).unwrap();
    let _ = shutdown.stop_services().unwrap();
    let _report = block_on(shutdown.wait_group_shutdown()).unwrap();
}

#[test]
fn test_concurrent_external_parent_children_share_one_budget_binding() {
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let port = Arc::new(TestExecution::default());
    let group = manager.group(qubit_operation::GroupOptions { capacity: 4 }).unwrap();
    let services = qubit_mime::MimeExecutionServices::new(manager.clone(), group, port, 16).unwrap();
    services.prepare().unwrap();
    let runtime = MimeRuntime::builtin()
        .unwrap()
        .with_execution_services(services.clone());
    let parent_options = qubit_operation::OperationOptions {
        limits: qubit_operation::ScopeLimits {
            child_operations: 4,
            resources: 1,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut parent = manager
        .as_async()
        .try_submit(parent_options, move |scope| {
            let body_runtime = runtime.clone();
            async move {
                let scope = Arc::new(scope);
                let barrier = Arc::new(std::sync::Barrier::new(3));
                let first_runtime = body_runtime.clone();
                let first_scope = Arc::clone(&scope);
                let first_barrier = Arc::clone(&barrier);
                let second_runtime = body_runtime.clone();
                let second_scope = Arc::clone(&scope);
                let second_barrier = Arc::clone(&barrier);
                let first = std::thread::spawn(move || {
                    first_barrier.wait();
                    first_runtime.try_submit_detection_in(
                        &first_scope,
                        OwnedMimeInput::bytes(Arc::from(b"%PDF".as_slice()), InputExtent::Complete),
                        MimeOperationOptions::default(),
                    )
                });
                let second = std::thread::spawn(move || {
                    second_barrier.wait();
                    second_runtime.try_submit_detection_in(
                        &second_scope,
                        OwnedMimeInput::bytes(Arc::from(b"text".as_slice()), InputExtent::Complete),
                        MimeOperationOptions::default(),
                    )
                });
                barrier.wait();
                let first = first.join().expect("first submission thread should finish");
                let second = second.join().expect("second submission thread should finish");
                let (Ok(mut first), Ok(mut second)) = (first, second) else {
                    return Ok::<_, ()>(false);
                };
                let first_outcome = first.wait_outcome().await.unwrap();
                let second_outcome = second.wait_outcome().await.unwrap();
                let _first_settlement = first.wait_settlement().await;
                let _second_settlement = second.wait_settlement().await;
                Ok::<_, ()>(
                    matches!(first_outcome, qubit_operation::OperationOutcome::Succeeded(_))
                        && matches!(second_outcome, qubit_operation::OperationOutcome::Succeeded(_)),
                )
            }
        })
        .unwrap();
    assert!(matches!(
        block_on(parent.wait_outcome()).unwrap(),
        qubit_operation::OperationOutcome::Succeeded(true)
    ));
    block_on(parent.wait_settlement());

    let shutdown = services.shutdown();
    let _ = block_on(shutdown.wait_services_settled()).unwrap();
    let _ = shutdown.stop_services().unwrap();
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
    let _ = block_on(shutdown.wait_services_settled()).unwrap();
    let _ = shutdown.stop_services().unwrap();
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
