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

#[derive(Debug)]
struct PrefixProvider {
    requirement: usize,
    observed: Arc<AtomicUsize>,
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
            observed: Arc::clone(&self.observed),
        }))
    }
}

#[derive(Debug)]
struct PrefixBackend {
    requirement: usize,
    observed: Arc<AtomicUsize>,
}

impl AsyncMimeDetectorBackend for PrefixBackend {
    fn content_requirement(&self) -> ContentRequirement {
        ContentRequirement::Prefix(self.requirement)
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
            let BorrowedMimeInput::ManagedProvider { session, .. } = input else {
                panic!("managed prefix backend must receive the opened read session")
            };
            let mut bytes = vec![0; self.requirement];
            let count = session.read(&mut bytes).await?;
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
        let registry = MimeDetectorRegistry::default();
        registry
            .register_async_provider(PrefixProvider {
                requirement: 8,
                observed: Arc::clone(&observed),
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
        let shutdown = services.shutdown();
        assert!(block_on(shutdown.wait_services_settled()).unwrap().operations_settled);
        assert!(shutdown.stop_services().unwrap().services_stopped);
    }
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
            observed,
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
                ProviderOperations::new().with(ProviderOperation::OpenReader),
                FileSystemCapabilities::new().with_guaranteed(FileSystemCapability::Read),
                FileSystemLimits::unknown(),
                PathConstraints::absolute(),
                SymlinkPolicy::Reject,
            )
            .unwrap(),
            content,
        }
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
        };
        Box::pin(async move { Ok(qubit_fs::spi::OpenedAsyncReader::new(info, Box::new(input))) })
    }
}

struct PrefixInput {
    content: Vec<u8>,
    offset: usize,
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
        let read = count.min(self.content.len().saturating_sub(self.offset));
        output[index..index + read].copy_from_slice(&self.content[self.offset..self.offset + read]);
        self.offset += read;
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
