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

use qubit_mime::AsyncMimeDetectorBackend;
use qubit_mime::AsyncMimeDetectorProvider;
use qubit_mime::BorrowedMimeInput;
use qubit_mime::ContentRequirement;
use qubit_mime::FixedMimeExecutionPort;
use qubit_mime::InputExtent;
use qubit_mime::InspectedExtent;
use qubit_mime::MimeCandidates;
use qubit_mime::MimeDetectorRegistry;
use qubit_mime::MimeError;
use qubit_mime::MimeExecutionServices;
use qubit_mime::MimeFuture;
use qubit_mime::MimeOperationOptions;
use qubit_mime::OwnedMimeInput;
use qubit_mime::RandomAccessCapability;
use qubit_mime::detector::MimeRuntime;
use qubit_spi::ProviderDescriptor;
use qubit_spi::ProviderId;
use qubit_spi::ProviderMetadata;

struct TestDeadlineClock {
    domain: qubit_operation::ClockDomainId,
    ticks: std::sync::atomic::AtomicU64,
}

impl qubit_operation::ClockSource for TestDeadlineClock {
    fn now(&self) -> qubit_operation::MonotonicTime {
        qubit_operation::MonotonicTime::new(self.domain, self.ticks.load(Ordering::Acquire))
    }

    fn register_deadline(
        &self,
        _at: qubit_operation::MonotonicTime,
        _wake: std::task::Waker,
    ) -> Box<dyn qubit_operation::TimerRegistration> {
        Box::new(TestTimerRegistration)
    }
}

struct TestTimerRegistration;

impl qubit_operation::TimerRegistration for TestTimerRegistration {
    fn cancel(self: Box<Self>) {}
}

#[derive(Debug)]
struct PrefixProvider {
    requirement: usize,
    managed_reads: bool,
    observed: Arc<AtomicUsize>,
    calls: Arc<AtomicUsize>,
}

impl ProviderMetadata for PrefixProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor::new(ProviderId::new("prefix-runtime-test").expect("provider ID"))
    }
}

impl AsyncMimeDetectorProvider for PrefixProvider {
    fn create_async_backend(
        &self,
        _config: &qubit_mime::MimeConfig,
        _services: &MimeExecutionServices,
    ) -> Result<Arc<dyn AsyncMimeDetectorBackend>, MimeError> {
        Ok(Arc::new(PrefixBackend {
            requirement: self.requirement,
            managed_reads: self.managed_reads,
            observed: Arc::clone(&self.observed),
            calls: Arc::clone(&self.calls),
        }))
    }
}

#[derive(Debug)]
struct PrefixBackend {
    requirement: usize,
    managed_reads: bool,
    observed: Arc<AtomicUsize>,
    calls: Arc<AtomicUsize>,
}

impl AsyncMimeDetectorBackend for PrefixBackend {
    fn content_requirement(&self) -> ContentRequirement {
        ContentRequirement::Prefix(self.requirement)
    }

    fn random_access_capability(&self) -> RandomAccessCapability {
        RandomAccessCapability::Unsupported
    }

    fn uses_managed_provider_reads(&self) -> bool {
        self.managed_reads
    }

    fn guess_content_in<'a>(
        &'a self,
        _scope: &'a qubit_operation::OperationScope,
        input: BorrowedMimeInput<'a>,
        _options: &'a MimeOperationOptions,
    ) -> MimeFuture<'a, MimeCandidates> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            let count = if self.managed_reads {
                let BorrowedMimeInput::ManagedProvider { session, .. } = input else {
                    panic!("managed prefix backend must receive the opened read session")
                };
                let mut bytes = vec![0; self.requirement];
                session.read(&mut bytes).await?
            } else {
                let BorrowedMimeInput::Bytes { content, extent, .. } = input else {
                    panic!("automatic prefix backend must receive bounded bytes")
                };
                assert_eq!(extent, InputExtent::Prefix);
                content.len()
            };
            self.observed.store(count, Ordering::SeqCst);
            Ok(MimeCandidates {
                candidates: Vec::new(),
                inspected_extent: InspectedExtent::Prefix(count),
                retained_prefix: Vec::new(),
                staging_bytes: 0,
            })
        })
    }
}

#[derive(Debug)]
struct CompleteProvider {
    observed: Arc<AtomicUsize>,
}

impl ProviderMetadata for CompleteProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor::new(ProviderId::new("complete-runtime-test").expect("provider ID"))
    }
}

impl AsyncMimeDetectorProvider for CompleteProvider {
    fn create_async_backend(
        &self,
        _config: &qubit_mime::MimeConfig,
        _services: &MimeExecutionServices,
    ) -> Result<Arc<dyn AsyncMimeDetectorBackend>, MimeError> {
        Ok(Arc::new(CompleteBackend {
            observed: Arc::clone(&self.observed),
        }))
    }
}

#[derive(Debug)]
struct CompleteBackend {
    observed: Arc<AtomicUsize>,
}

struct ParentChildAccountingProvider {
    managed_reads: bool,
    submit_children: bool,
    child_runtime: Arc<Mutex<Option<MimeRuntime>>>,
    file_system: Arc<qubit_fs::AsyncFileSystem>,
    child_report_bytes: Arc<Mutex<Vec<u64>>>,
}

impl std::fmt::Debug for ParentChildAccountingProvider {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ParentChildAccountingProvider")
            .field("managed_reads", &self.managed_reads)
            .finish_non_exhaustive()
    }
}

impl ProviderMetadata for ParentChildAccountingProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor::new(ProviderId::new("parent-child-accounting-test").expect("provider ID"))
    }
}

impl AsyncMimeDetectorProvider for ParentChildAccountingProvider {
    fn create_async_backend(
        &self,
        _config: &qubit_mime::MimeConfig,
        _services: &MimeExecutionServices,
    ) -> Result<Arc<dyn AsyncMimeDetectorBackend>, MimeError> {
        Ok(Arc::new(ParentChildAccountingBackend {
            managed_reads: self.managed_reads,
            submit_children: self.submit_children,
            child_runtime: Arc::clone(&self.child_runtime),
            file_system: Arc::clone(&self.file_system),
            child_report_bytes: Arc::clone(&self.child_report_bytes),
        }))
    }
}

struct ParentChildAccountingBackend {
    managed_reads: bool,
    submit_children: bool,
    child_runtime: Arc<Mutex<Option<MimeRuntime>>>,
    file_system: Arc<qubit_fs::AsyncFileSystem>,
    child_report_bytes: Arc<Mutex<Vec<u64>>>,
}

impl std::fmt::Debug for ParentChildAccountingBackend {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ParentChildAccountingBackend")
            .field("managed_reads", &self.managed_reads)
            .finish_non_exhaustive()
    }
}

impl AsyncMimeDetectorBackend for ParentChildAccountingBackend {
    fn content_requirement(&self) -> ContentRequirement {
        ContentRequirement::Prefix(4)
    }

    fn random_access_capability(&self) -> RandomAccessCapability {
        RandomAccessCapability::Unsupported
    }

    fn uses_managed_provider_reads(&self) -> bool {
        self.managed_reads
    }

    fn guess_content_in<'a>(
        &'a self,
        scope: &'a qubit_operation::OperationScope,
        input: BorrowedMimeInput<'a>,
        _options: &'a MimeOperationOptions,
    ) -> MimeFuture<'a, MimeCandidates> {
        Box::pin(async move {
            let parent = match input {
                BorrowedMimeInput::Bytes { filename, .. } => filename == Some("parent"),
                BorrowedMimeInput::ManagedProvider { filename, session } => {
                    if filename == Some("parent") {
                        for _ in 0..2 {
                            let mut bytes = [0; 4];
                            session.read(&mut bytes).await?;
                        }
                        true
                    } else {
                        let reads = if filename == Some("child-a") { 2 } else { 1 };
                        for _ in 0..reads {
                            let mut bytes = [0; 4];
                            session.read(&mut bytes).await?;
                        }
                        false
                    }
                }
                BorrowedMimeInput::Provider { .. } => {
                    panic!("the test backend should only see bytes or managed provider input")
                }
            };

            if parent && self.submit_children {
                let runtime = self
                    .child_runtime
                    .lock()
                    .unwrap()
                    .clone()
                    .expect("test runtime installed before parent submission");
                for name in ["child-a", "child-b"] {
                    let mut child = runtime
                        .try_submit_detection_in(
                            scope,
                            OwnedMimeInput::Provider {
                                file_system: Arc::clone(&self.file_system),
                                path: qubit_fs::Path::parse(&format!("/{name}")).unwrap(),
                            },
                            MimeOperationOptions {
                                max_buffer_bytes: Some(4),
                                max_read_bytes: Some(32),
                                ..Default::default()
                            },
                        )
                        .unwrap_or_else(|failure| panic!("{name} child rejected: {:?}", failure.reason));
                    let report = match child
                        .wait_outcome()
                        .await
                        .unwrap_or_else(|error| panic!("{name} child outcome unavailable: {error:?}"))
                    {
                        qubit_operation::OperationOutcome::Succeeded(report) => report,
                        other => panic!("{name} child should succeed: {other:?}"),
                    };
                    self.child_report_bytes.lock().unwrap().push(report.budget.bytes_read);
                }
            }

            Ok(MimeCandidates {
                candidates: Vec::new(),
                inspected_extent: InspectedExtent::Prefix(4),
                retained_prefix: Vec::new(),
                staging_bytes: 0,
            })
        })
    }
}

#[derive(Debug, Clone, Copy)]
enum ParentInputRoute {
    Bytes,
    AutomaticPrefix,
    ManagedProvider,
}

impl AsyncMimeDetectorBackend for CompleteBackend {
    fn content_requirement(&self) -> ContentRequirement {
        ContentRequirement::Complete
    }

    fn random_access_capability(&self) -> RandomAccessCapability {
        RandomAccessCapability::SamplesSameObject
    }

    fn uses_managed_provider_reads(&self) -> bool {
        true
    }

    fn guess_content_in<'a>(
        &'a self,
        _scope: &'a qubit_operation::OperationScope,
        _input: BorrowedMimeInput<'a>,
        _options: &'a MimeOperationOptions,
    ) -> MimeFuture<'a, MimeCandidates> {
        Box::pin(async { panic!("complete Provider backend should receive the same-object read session") })
    }

    fn guess_provider_content_in<'a>(
        &'a self,
        _scope: &'a qubit_operation::OperationScope,
        input: BorrowedMimeInput<'a>,
        _filename: Option<&'a str>,
        _options: &'a MimeOperationOptions,
    ) -> MimeFuture<'a, MimeCandidates> {
        Box::pin(async move {
            let BorrowedMimeInput::ManagedProvider { session, .. } = input else {
                panic!("complete Provider backend must receive managed reads")
            };
            let mut first = [0; 4];
            let mut second = [0; 4];
            let first_count = session.read_at(0, &mut first).await?;
            let second_count = session.read_at(0, &mut second).await?;
            self.observed.store(first_count + second_count, Ordering::SeqCst);
            Ok(MimeCandidates {
                candidates: Vec::new(),
                inspected_extent: InspectedExtent::RandomAccessSamples,
                retained_prefix: Vec::new(),
                staging_bytes: 0,
            })
        })
    }
}

#[test]
fn test_managed_prefix_backend_observes_static_cap_and_natural_eof() {
    for (name, content, cap, expected) in [
        ("static-cap", b"abcdefgh".as_slice(), 4usize, 4usize),
        ("natural-eof", b"abc".as_slice(), 8usize, 3usize),
    ] {
        let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
        let group = manager.group(qubit_operation::GroupOptions { capacity: 2 }).unwrap();
        let services =
            MimeExecutionServices::new(manager, group, Arc::new(FixedMimeExecutionPort::new(1, 2)), 1024).unwrap();
        services.prepare().unwrap();
        let observed = Arc::new(AtomicUsize::new(usize::MAX));
        let calls = Arc::new(AtomicUsize::new(0));
        let registry = MimeDetectorRegistry::default();
        registry
            .register_async_provider(PrefixProvider {
                requirement: 8,
                managed_reads: true,
                observed: Arc::clone(&observed),
                calls: Arc::clone(&calls),
            })
            .unwrap();
        let runtime = MimeRuntime::builtin()
            .unwrap()
            .with_execution_services(services.clone())
            .with_async_provider_selection(
                &registry,
                &qubit_spi::ProviderSelection::named("prefix-runtime-test").unwrap(),
            )
            .unwrap();
        let filesystem = qubit_fs::AsyncFileSystem::from_spi(PrefixFileSystem::new(content.to_vec())).unwrap();
        let mut operation = runtime
            .try_submit_detection(
                OwnedMimeInput::Provider {
                    file_system: Arc::new(filesystem),
                    path: qubit_fs::Path::parse("/input").unwrap(),
                },
                MimeOperationOptions {
                    max_buffer_bytes: Some(cap),
                    max_read_bytes: Some(cap as u64),
                    ..Default::default()
                },
            )
            .unwrap_or_else(|failure| panic!("{name} prefix operation rejected: {:?}", failure.reason));

        match block_on(operation.wait_outcome()).unwrap() {
            qubit_operation::OperationOutcome::Succeeded(report) => {
                assert_eq!(report.inspected_extent, InspectedExtent::Prefix(expected));
            }
            other => panic!("{name} prefix operation failed: {other:?}"),
        }
        assert_eq!(
            observed.load(Ordering::SeqCst),
            expected,
            "backend input length for {name}"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1, "positive control for {name}");
        let shutdown = services.shutdown();
        assert!(block_on(shutdown.wait_services_settled()).unwrap().operations_settled);
        assert!(shutdown.stop_services().unwrap().services_stopped);
    }
}

#[test]
fn test_automatic_prefix_backend_observes_static_cap_and_natural_eof() {
    for (name, content, cap, expected) in [
        ("static-cap", b"abcdefgh".as_slice(), 4usize, 4usize),
        ("natural-eof", b"abc".as_slice(), 8usize, 3usize),
    ] {
        let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
        let group = manager.group(qubit_operation::GroupOptions { capacity: 2 }).unwrap();
        let services =
            MimeExecutionServices::new(manager, group, Arc::new(FixedMimeExecutionPort::new(1, 2)), 1024).unwrap();
        services.prepare().unwrap();
        let observed = Arc::new(AtomicUsize::new(usize::MAX));
        let calls = Arc::new(AtomicUsize::new(0));
        let registry = MimeDetectorRegistry::default();
        registry
            .register_async_provider(PrefixProvider {
                requirement: 8,
                managed_reads: false,
                observed: Arc::clone(&observed),
                calls: Arc::clone(&calls),
            })
            .unwrap();
        let runtime = MimeRuntime::builtin()
            .unwrap()
            .with_execution_services(services.clone())
            .with_async_provider_selection(
                &registry,
                &qubit_spi::ProviderSelection::named("prefix-runtime-test").unwrap(),
            )
            .unwrap();
        let filesystem = qubit_fs::AsyncFileSystem::from_spi(PrefixFileSystem::new(content.to_vec())).unwrap();
        let mut operation = runtime
            .try_submit_detection(
                OwnedMimeInput::Provider {
                    file_system: Arc::new(filesystem),
                    path: qubit_fs::Path::parse("/input").unwrap(),
                },
                MimeOperationOptions {
                    max_buffer_bytes: Some(cap),
                    max_read_bytes: Some(cap as u64),
                    ..Default::default()
                },
            )
            .unwrap_or_else(|failure| panic!("{name} automatic prefix operation rejected: {:?}", failure.reason));

        match block_on(operation.wait_outcome()).unwrap() {
            qubit_operation::OperationOutcome::Succeeded(report) => {
                assert_eq!(report.inspected_extent, InspectedExtent::Prefix(expected));
                assert_eq!(report.budget.bytes_read, expected as u64);
            }
            other => panic!("{name} automatic prefix operation failed: {other:?}"),
        }
        assert_eq!(
            observed.load(Ordering::SeqCst),
            expected,
            "backend input length for {name}"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1, "positive control for {name}");
        let shutdown = services.shutdown();
        assert!(block_on(shutdown.wait_services_settled()).unwrap().operations_settled);
        assert!(shutdown.stop_services().unwrap().services_stopped);
    }
}

#[test]
fn test_complete_managed_provider_reports_repeated_same_object_reads() {
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let group = manager.group(qubit_operation::GroupOptions { capacity: 2 }).unwrap();
    let services =
        MimeExecutionServices::new(manager, group, Arc::new(FixedMimeExecutionPort::new(1, 2)), 1024).unwrap();
    services.prepare().unwrap();
    let observed = Arc::new(AtomicUsize::new(usize::MAX));
    let registry = MimeDetectorRegistry::default();
    registry
        .register_async_provider(CompleteProvider {
            observed: Arc::clone(&observed),
        })
        .unwrap();
    let runtime = MimeRuntime::builtin()
        .unwrap()
        .with_execution_services(services.clone())
        .with_async_provider_selection(
            &registry,
            &qubit_spi::ProviderSelection::named("complete-runtime-test").unwrap(),
        )
        .unwrap();
    let filesystem = qubit_fs::AsyncFileSystem::from_spi(PrefixFileSystem::new(b"abcdefgh".to_vec())).unwrap();
    let mut operation = runtime
        .try_submit_detection(
            OwnedMimeInput::Provider {
                file_system: Arc::new(filesystem),
                path: qubit_fs::Path::parse("/input").unwrap(),
            },
            MimeOperationOptions {
                max_buffer_bytes: Some(4),
                max_read_bytes: Some(8),
                ..Default::default()
            },
        )
        .unwrap_or_else(|failure| panic!("complete Provider operation rejected: {:?}", failure.reason));

    match block_on(operation.wait_outcome()).unwrap() {
        qubit_operation::OperationOutcome::Succeeded(report) => {
            assert_eq!(report.inspected_extent, InspectedExtent::RandomAccessSamples);
            assert_eq!(report.budget.bytes_read, 8);
        }
        other => panic!("complete Provider operation failed: {other:?}"),
    }
    assert_eq!(
        8,
        observed.load(Ordering::SeqCst),
        "overlapping successful reads count twice"
    );
    let shutdown = services.shutdown();
    assert!(block_on(shutdown.wait_services_settled()).unwrap().operations_settled);
    assert!(shutdown.stop_services().unwrap().services_stopped);
}

#[test]
fn test_parent_and_child_reports_count_local_bytes_across_input_routes() {
    for (route, parent_bytes, root_limit, managed_reads) in [
        (ParentInputRoute::Bytes, 0u64, 12u64, false),
        (ParentInputRoute::AutomaticPrefix, 4, 16, false),
        (ParentInputRoute::ManagedProvider, 8, 20, true),
    ] {
        let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
        let group = manager.group(qubit_operation::GroupOptions { capacity: 4 }).unwrap();
        let services =
            MimeExecutionServices::new(manager, group, Arc::new(FixedMimeExecutionPort::new(1, 2)), 32).unwrap();
        services.prepare().unwrap();
        let file_system =
            Arc::new(qubit_fs::AsyncFileSystem::from_spi(PrefixFileSystem::new(b"abcdefgh".to_vec())).unwrap());
        let child_runtime = Arc::new(Mutex::new(None));
        let child_report_bytes = Arc::new(Mutex::new(Vec::new()));
        let child_registry = MimeDetectorRegistry::default();
        child_registry
            .register_async_provider(ParentChildAccountingProvider {
                managed_reads: true,
                submit_children: false,
                child_runtime: Arc::new(Mutex::new(None)),
                file_system: Arc::clone(&file_system),
                child_report_bytes: Arc::clone(&child_report_bytes),
            })
            .unwrap();
        let child_runtime_instance = MimeRuntime::builtin()
            .unwrap()
            .with_execution_services(services.clone())
            .with_async_provider_selection(
                &child_registry,
                &qubit_spi::ProviderSelection::named("parent-child-accounting-test").unwrap(),
            )
            .unwrap();
        *child_runtime.lock().unwrap() = Some(child_runtime_instance);
        let registry = MimeDetectorRegistry::default();
        registry
            .register_async_provider(ParentChildAccountingProvider {
                managed_reads,
                submit_children: true,
                child_runtime: Arc::clone(&child_runtime),
                file_system: Arc::clone(&file_system),
                child_report_bytes: Arc::clone(&child_report_bytes),
            })
            .unwrap();
        let runtime = MimeRuntime::builtin()
            .unwrap()
            .with_execution_services(services.clone())
            .with_async_provider_selection(
                &registry,
                &qubit_spi::ProviderSelection::named("parent-child-accounting-test").unwrap(),
            )
            .unwrap();

        let input = match route {
            ParentInputRoute::Bytes => {
                OwnedMimeInput::bytes(Arc::from(&b"abcd"[..]), InputExtent::Complete).with_filename("parent")
            }
            ParentInputRoute::AutomaticPrefix | ParentInputRoute::ManagedProvider => OwnedMimeInput::Provider {
                file_system: Arc::clone(&file_system),
                path: qubit_fs::Path::parse("/parent").unwrap(),
            },
        };
        let mut parent = runtime
            .try_submit_detection(
                input,
                MimeOperationOptions {
                    max_buffer_bytes: Some(4),
                    max_read_bytes: Some(root_limit),
                    ..Default::default()
                },
            )
            .unwrap_or_else(|failure| panic!("parent route rejected: {:?}", failure.reason));
        let report = match block_on(parent.wait_outcome()).unwrap() {
            qubit_operation::OperationOutcome::Succeeded(report) => report,
            other => panic!("parent route should succeed: {other:?}"),
        };

        assert_eq!(parent_bytes, report.budget.bytes_read, "parent own bytes for {route:?}");
        assert_eq!(
            vec![8, 4],
            *child_report_bytes.lock().unwrap(),
            "child reports must count their own repeated reads and share the parent's remaining quota"
        );
        *child_runtime.lock().unwrap() = None;
        let shutdown = services.shutdown();
        assert!(block_on(shutdown.wait_services_settled()).unwrap().operations_settled);
        assert!(shutdown.stop_services().unwrap().services_stopped);
    }
}

#[test]
fn test_automatic_prefix_stops_before_second_read_after_first_read_requests_stop() {
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let group = manager.group(qubit_operation::GroupOptions { capacity: 2 }).unwrap();
    let services =
        MimeExecutionServices::new(manager, group, Arc::new(FixedMimeExecutionPort::new(1, 2)), 1024).unwrap();
    services.prepare().unwrap();
    let registry = MimeDetectorRegistry::default();
    registry
        .register_async_provider(PrefixProvider {
            requirement: 8,
            managed_reads: false,
            observed: Arc::new(AtomicUsize::new(usize::MAX)),
            calls: Arc::new(AtomicUsize::new(0)),
        })
        .unwrap();
    let runtime = MimeRuntime::builtin()
        .unwrap()
        .with_execution_services(services.clone())
        .with_async_provider_selection(
            &registry,
            &qubit_spi::ProviderSelection::named("prefix-runtime-test").unwrap(),
        )
        .unwrap();
    let (read_tx, read_rx) = mpsc::channel();
    let (continue_tx, continue_rx) = mpsc::channel();
    let continue_rx = Mutex::new(continue_rx);
    let hook: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        read_tx.send(()).unwrap();
        continue_rx.lock().unwrap().recv().unwrap();
    });
    let filesystem = PrefixFileSystem::new(b"abcdefgh".to_vec()).with_first_read_hook(1, hook);
    let read_calls = Arc::clone(&filesystem.read_calls);
    let filesystem = qubit_fs::AsyncFileSystem::from_spi(filesystem).unwrap();
    let mut operation = runtime
        .try_submit_detection(
            OwnedMimeInput::Provider {
                file_system: Arc::new(filesystem),
                path: qubit_fs::Path::parse("/input").unwrap(),
            },
            MimeOperationOptions {
                max_buffer_bytes: Some(8),
                max_read_bytes: Some(8),
                ..Default::default()
            },
        )
        .unwrap_or_else(|failure| panic!("automatic prefix stop operation rejected: {:?}", failure.reason));

    read_rx.recv().unwrap();
    assert_eq!(
        qubit_operation::StopRequestStatus::Accepted,
        operation.request_stop(qubit_operation::StopReason::UserCancelled)
    );
    continue_tx.send(()).unwrap();
    match block_on(operation.wait_outcome()).unwrap() {
        qubit_operation::OperationOutcome::Stopped(report) => {
            assert_eq!(report.reason, qubit_operation::StopReason::UserCancelled);
        }
        other => panic!("automatic prefix should preserve the stop outcome: {other:?}"),
    }
    assert_eq!(
        1,
        read_calls.load(Ordering::SeqCst),
        "no second provider read starts after stop"
    );
    let shutdown = services.shutdown();
    assert!(block_on(shutdown.wait_services_settled()).unwrap().operations_settled);
    assert!(shutdown.stop_services().unwrap().services_stopped);
}

#[test]
fn test_managed_session_open_deadline_skips_extension_backend() {
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let group = manager.group(qubit_operation::GroupOptions { capacity: 2 }).unwrap();
    let services =
        MimeExecutionServices::new(manager, group, Arc::new(FixedMimeExecutionPort::new(1, 2)), 1024).unwrap();
    services.prepare().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let registry = MimeDetectorRegistry::default();
    registry
        .register_async_provider(PrefixProvider {
            requirement: 8,
            managed_reads: true,
            observed: Arc::new(AtomicUsize::new(usize::MAX)),
            calls: Arc::clone(&calls),
        })
        .unwrap();
    let runtime = MimeRuntime::builtin()
        .unwrap()
        .with_execution_services(services.clone())
        .with_async_provider_selection(
            &registry,
            &qubit_spi::ProviderSelection::named("prefix-runtime-test").unwrap(),
        )
        .unwrap();
    let domain = qubit_operation::ClockDomainId::new_unique();
    let clock = Arc::new(TestDeadlineClock {
        domain,
        ticks: std::sync::atomic::AtomicU64::new(0),
    });
    let clock_after_open = Arc::clone(&clock);
    let hook: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        clock_after_open.ticks.store(5, Ordering::Release);
    });
    let filesystem = PrefixFileSystem::new(b"abcdefgh".to_vec()).with_open_hook(hook);
    let filesystem = qubit_fs::AsyncFileSystem::from_spi(filesystem).unwrap();
    let mut operation = runtime
        .try_submit_detection(
            OwnedMimeInput::Provider {
                file_system: Arc::new(filesystem),
                path: qubit_fs::Path::parse("/input").unwrap(),
            },
            MimeOperationOptions {
                max_buffer_bytes: Some(8),
                max_read_bytes: Some(8),
                operation: qubit_operation::OperationOptions {
                    context: qubit_operation::ContextOptions {
                        clock: Some(clock),
                        deadline: Some(qubit_operation::Deadline::at(qubit_operation::MonotonicTime::new(
                            domain, 5,
                        ))),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .unwrap_or_else(|failure| panic!("managed open deadline operation rejected: {:?}", failure.reason));

    match block_on(operation.wait_outcome()).unwrap() {
        qubit_operation::OperationOutcome::Stopped(report) => {
            assert_eq!(report.reason, qubit_operation::StopReason::Timeout);
        }
        other => panic!("managed open should preserve the deadline outcome: {other:?}"),
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0, "extension backend must not be called");
    let shutdown = services.shutdown();
    assert!(block_on(shutdown.wait_services_settled()).unwrap().operations_settled);
    assert!(shutdown.stop_services().unwrap().services_stopped);
}

#[test]
fn test_automatic_prefix_deadline_after_final_read_skips_extension_backend() {
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let group = manager.group(qubit_operation::GroupOptions { capacity: 2 }).unwrap();
    let services =
        MimeExecutionServices::new(manager, group, Arc::new(FixedMimeExecutionPort::new(1, 2)), 1024).unwrap();
    services.prepare().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let registry = MimeDetectorRegistry::default();
    registry
        .register_async_provider(PrefixProvider {
            requirement: 4,
            managed_reads: false,
            observed: Arc::new(AtomicUsize::new(usize::MAX)),
            calls: Arc::clone(&calls),
        })
        .unwrap();
    let runtime = MimeRuntime::builtin()
        .unwrap()
        .with_execution_services(services.clone())
        .with_async_provider_selection(
            &registry,
            &qubit_spi::ProviderSelection::named("prefix-runtime-test").unwrap(),
        )
        .unwrap();
    let domain = qubit_operation::ClockDomainId::new_unique();
    let clock = Arc::new(TestDeadlineClock {
        domain,
        ticks: std::sync::atomic::AtomicU64::new(0),
    });
    let clock_after_read = Arc::clone(&clock);
    let hook: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        clock_after_read.ticks.store(5, Ordering::Release);
    });
    let filesystem = PrefixFileSystem::new(b"abcdefgh".to_vec()).with_first_read_hook(4, hook);
    let read_calls = Arc::clone(&filesystem.read_calls);
    let filesystem = qubit_fs::AsyncFileSystem::from_spi(filesystem).unwrap();
    let mut operation = runtime
        .try_submit_detection(
            OwnedMimeInput::Provider {
                file_system: Arc::new(filesystem),
                path: qubit_fs::Path::parse("/input").unwrap(),
            },
            MimeOperationOptions {
                max_buffer_bytes: Some(4),
                max_read_bytes: Some(4),
                operation: qubit_operation::OperationOptions {
                    context: qubit_operation::ContextOptions {
                        clock: Some(clock),
                        deadline: Some(qubit_operation::Deadline::at(qubit_operation::MonotonicTime::new(
                            domain, 5,
                        ))),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .unwrap_or_else(|failure| {
            panic!(
                "automatic prefix final-read deadline operation rejected: {:?}",
                failure.reason
            )
        });

    match block_on(operation.wait_outcome()).unwrap() {
        qubit_operation::OperationOutcome::Stopped(report) => {
            assert_eq!(report.reason, qubit_operation::StopReason::Timeout);
        }
        other => panic!("automatic prefix should preserve the deadline outcome: {other:?}"),
    }
    assert_eq!(
        read_calls.load(Ordering::SeqCst),
        1,
        "the full prefix was delivered in one read"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0, "extension backend must not be called");
    let shutdown = services.shutdown();
    assert!(block_on(shutdown.wait_services_settled()).unwrap().operations_settled);
    assert!(shutdown.stop_services().unwrap().services_stopped);
}

#[test]
fn test_automatic_prefix_deadline_after_first_read_blocks_second_read() {
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let group = manager.group(qubit_operation::GroupOptions { capacity: 2 }).unwrap();
    let services =
        MimeExecutionServices::new(manager, group, Arc::new(FixedMimeExecutionPort::new(1, 2)), 1024).unwrap();
    services.prepare().unwrap();
    let registry = MimeDetectorRegistry::default();
    registry
        .register_async_provider(PrefixProvider {
            requirement: 8,
            managed_reads: false,
            observed: Arc::new(AtomicUsize::new(usize::MAX)),
            calls: Arc::new(AtomicUsize::new(0)),
        })
        .unwrap();
    let runtime = MimeRuntime::builtin()
        .unwrap()
        .with_execution_services(services.clone())
        .with_async_provider_selection(
            &registry,
            &qubit_spi::ProviderSelection::named("prefix-runtime-test").unwrap(),
        )
        .unwrap();
    let domain = qubit_operation::ClockDomainId::new_unique();
    let clock = Arc::new(TestDeadlineClock {
        domain,
        ticks: std::sync::atomic::AtomicU64::new(0),
    });
    let clock_after_read = Arc::clone(&clock);
    let hook: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        clock_after_read.ticks.store(5, Ordering::Release);
    });
    let filesystem = PrefixFileSystem::new(b"abcdefgh".to_vec()).with_first_read_hook(1, hook);
    let read_calls = Arc::clone(&filesystem.read_calls);
    let filesystem = qubit_fs::AsyncFileSystem::from_spi(filesystem).unwrap();
    let mut operation = runtime
        .try_submit_detection(
            OwnedMimeInput::Provider {
                file_system: Arc::new(filesystem),
                path: qubit_fs::Path::parse("/input").unwrap(),
            },
            MimeOperationOptions {
                max_buffer_bytes: Some(8),
                max_read_bytes: Some(8),
                operation: qubit_operation::OperationOptions {
                    context: qubit_operation::ContextOptions {
                        clock: Some(clock),
                        deadline: Some(qubit_operation::Deadline::at(qubit_operation::MonotonicTime::new(
                            domain, 5,
                        ))),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .unwrap_or_else(|failure| panic!("automatic prefix deadline operation rejected: {:?}", failure.reason));

    match block_on(operation.wait_outcome()).unwrap() {
        qubit_operation::OperationOutcome::Stopped(report) => {
            assert_eq!(report.reason, qubit_operation::StopReason::Timeout);
        }
        other => panic!("automatic prefix should preserve the deadline outcome: {other:?}"),
    }
    assert_eq!(
        1,
        read_calls.load(Ordering::SeqCst),
        "no second provider read starts after deadline"
    );
    let shutdown = services.shutdown();
    assert!(block_on(shutdown.wait_services_settled()).unwrap().operations_settled);
    assert!(shutdown.stop_services().unwrap().services_stopped);
}

#[test]
fn test_managed_prefix_owned_budget_rejection_returns_prefix_input() {
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let group = manager.group(qubit_operation::GroupOptions { capacity: 2 }).unwrap();
    let services =
        MimeExecutionServices::new(manager, group, Arc::new(FixedMimeExecutionPort::new(1, 2)), 1024).unwrap();
    services.prepare().unwrap();
    let observed = Arc::new(AtomicUsize::new(usize::MAX));
    let registry = MimeDetectorRegistry::default();
    registry
        .register_async_provider(PrefixProvider {
            requirement: 8,
            managed_reads: true,
            observed,
            calls: Arc::new(AtomicUsize::new(0)),
        })
        .unwrap();
    let runtime = MimeRuntime::builtin()
        .unwrap()
        .with_execution_services(services.clone())
        .with_async_provider_selection(
            &registry,
            &qubit_spi::ProviderSelection::named("prefix-runtime-test").unwrap(),
        )
        .unwrap();
    let bytes: Arc<[u8]> = vec![0; 16 * 1024 * 1024 + 1].into();
    let failure = match runtime.try_submit_detection(
        OwnedMimeInput::bytes(Arc::clone(&bytes), InputExtent::Prefix),
        MimeOperationOptions::default(),
    ) {
        Ok(_) => panic!("oversized owned prefix must be rejected before admission"),
        Err(failure) => failure,
    };

    assert!(matches!(
        failure.reason,
        qubit_mime::MimeSubmitRejection::BudgetExceeded
    ));
    match failure.input {
        OwnedMimeInput::Bytes { content, extent, .. } => {
            assert!(Arc::ptr_eq(&bytes, &content));
            assert_eq!(extent, InputExtent::Prefix);
        }
        OwnedMimeInput::Provider { .. } => panic!("rejected owned input changed kind"),
    }
    let shutdown = services.shutdown();
    assert!(block_on(shutdown.wait_services_settled()).unwrap().operations_settled);
    assert!(shutdown.stop_services().unwrap().services_stopped);
}

struct PrefixFileSystem {
    properties: qubit_fs::spi::ProviderProperties,
    content: Vec<u8>,
    max_read: usize,
    read_calls: Arc<AtomicUsize>,
    first_read_hook: Option<Arc<dyn Fn() + Send + Sync>>,
    open_hook: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl PrefixFileSystem {
    fn new(content: Vec<u8>) -> Self {
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

        Self {
            properties: qubit_fs::spi::ProviderProperties::new(
                FileSystemInfo::new(
                    FileSystemId::new("prefix-runtime-filesystem").unwrap(),
                    "prefix-runtime-filesystem",
                    PathSemantics::Hierarchical,
                ),
                ProviderOperations::new()
                    .with(ProviderOperation::OpenReader)
                    .with(ProviderOperation::OpenRandomAccessReader),
                FileSystemCapabilities::new().with_guaranteed(FileSystemCapability::Read),
                FileSystemLimits::unknown(),
                PathConstraints::absolute(),
                SymlinkPolicy::Reject,
            )
            .unwrap(),
            content,
            max_read: usize::MAX,
            read_calls: Arc::new(AtomicUsize::new(0)),
            first_read_hook: None,
            open_hook: None,
        }
    }

    fn with_first_read_hook(mut self, max_read: usize, hook: Arc<dyn Fn() + Send + Sync>) -> Self {
        self.max_read = max_read;
        self.first_read_hook = Some(hook);
        self
    }

    fn with_open_hook(mut self, hook: Arc<dyn Fn() + Send + Sync>) -> Self {
        self.open_hook = Some(hook);
        self
    }
}

impl qubit_fs::spi::AsyncFileSystemSpi for PrefixFileSystem {
    fn properties(&self) -> qubit_fs::spi::ProviderProperties {
        self.properties.clone()
    }

    fn stat<'a>(
        &'a self,
        _request: qubit_fs::spi::StatRequest<'a>,
    ) -> qubit_fs::spi::SpiFuture<'a, qubit_fs::FsResult<qubit_fs::spi::StatResponse>> {
        Box::pin(async { panic!("managed prefix reading must not stat provider input") })
    }

    fn open_reader<'a>(
        &'a self,
        request: qubit_fs::spi::OpenReaderRequest<'a>,
    ) -> qubit_fs::spi::SpiFuture<'a, qubit_fs::FsResult<qubit_fs::spi::OpenedAsyncReader>> {
        let info = qubit_fs::metadata::OpenedFileInfo::new(
            qubit_fs::metadata::FileSystemId::new("prefix-runtime-filesystem").unwrap(),
            request.path().clone(),
        );
        let input = PrefixInput {
            content: self.content.clone(),
            offset: 0,
            max_read: self.max_read,
            read_calls: Arc::clone(&self.read_calls),
            first_read_hook: self.first_read_hook.clone(),
        };
        let open_hook = self.open_hook.clone();
        Box::pin(async move {
            if let Some(hook) = open_hook {
                hook();
            }
            Ok(qubit_fs::spi::OpenedAsyncReader::new(info, Box::new(input)))
        })
    }

    fn open_random_access_reader<'a>(
        &'a self,
        request: qubit_fs::spi::OpenReaderRequest<'a>,
    ) -> qubit_fs::spi::SpiFuture<'a, qubit_fs::FsResult<qubit_fs::spi::OpenedAsyncRandomAccessReader>> {
        let info = qubit_fs::metadata::OpenedFileInfo::new(
            qubit_fs::metadata::FileSystemId::new("prefix-runtime-filesystem").unwrap(),
            request.path().clone(),
        )
        .with_metadata(
            qubit_fs::metadata::FileMetadata::new(qubit_fs::metadata::FileKind::File)
                .with_len(Some(self.content.len() as u64)),
        );
        let session = PrefixRandomAccessSession {
            content: self.content.clone(),
        };
        Box::pin(async move {
            Ok(qubit_fs::spi::OpenedAsyncRandomAccessReader::new(
                info,
                Box::new(session),
            ))
        })
    }
}

struct PrefixRandomAccessSession {
    content: Vec<u8>,
}

impl qubit_fs::spi::AsyncRandomAccessReadSession for PrefixRandomAccessSession {
    fn read_at_async<'a>(
        self: Pin<&'a mut Self>,
        offset: u64,
        output: &'a mut [u8],
    ) -> qubit_fs::spi::SpiFuture<'a, qubit_fs::FsResult<usize>> {
        let this = self.get_mut();
        let start = usize::try_from(offset).unwrap_or(usize::MAX);
        let count = output.len().min(this.content.len().saturating_sub(start));
        output[..count].copy_from_slice(&this.content[start..start + count]);
        Box::pin(async move { Ok(count) })
    }
}

struct PrefixInput {
    content: Vec<u8>,
    offset: usize,
    max_read: usize,
    read_calls: Arc<AtomicUsize>,
    first_read_hook: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl qubit_io::AsyncInput for PrefixInput {
    type Item = u8;

    unsafe fn poll_read_unchecked(
        mut self: Pin<&mut Self>,
        _context: &mut Context<'_>,
        output: &mut [u8],
        index: usize,
        count: usize,
    ) -> Poll<std::io::Result<usize>> {
        let read = count
            .min(self.max_read)
            .min(self.content.len().saturating_sub(self.offset));
        output[index..index + read].copy_from_slice(&self.content[self.offset..self.offset + read]);
        self.offset += read;
        if self.read_calls.fetch_add(1, Ordering::SeqCst) == 0
            && let Some(hook) = &self.first_read_hook
        {
            hook();
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
            Poll::Ready(output) => return output,
            Poll::Pending => std::thread::park(),
        }
    }
}
