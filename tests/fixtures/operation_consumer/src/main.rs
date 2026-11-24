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

use qubit_mime::AsyncMimeDetectorBackend;
use qubit_mime::AsyncMimeDetectorProvider;
use qubit_mime::BorrowedMimeInput;
use qubit_mime::ContentRequirement;
use qubit_mime::FixedMimeExecutionPort;
use qubit_mime::InspectedExtent;
use qubit_mime::MimeCandidate;
use qubit_mime::MimeCandidates;
use qubit_mime::MimeConfig;
use qubit_mime::MimeDetectorRegistry;
use qubit_mime::MimeError;
use qubit_mime::MimeExecutionServices;
use qubit_mime::MimeFuture;
use qubit_mime::MimeOperationOptions;
use qubit_mime::OwnedMimeInput;
use qubit_mime::RandomAccessCapability;
use qubit_mime::detector::MimeRuntime;
use qubit_operation::OperationManager;
use qubit_spi::ProviderDescriptor;
use qubit_spi::ProviderId;
use qubit_spi::ProviderMetadata;

#[derive(Debug)]
struct ConsumerProvider(bool);

impl ProviderMetadata for ConsumerProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        if self.0 {
            ProviderDescriptor::new(ProviderId::new("samples").unwrap())
        } else {
            ProviderDescriptor::new(ProviderId::new("complete").unwrap())
        }
    }
}

impl AsyncMimeDetectorProvider for ConsumerProvider {
    fn create_async_backend(
        &self,
        _config: &MimeConfig,
        _services: &MimeExecutionServices,
    ) -> Result<Arc<dyn AsyncMimeDetectorBackend>, MimeError> {
        Ok(Arc::new(ConsumerBackend(self.0)))
    }
}

#[derive(Debug)]
struct ConsumerBackend(bool);

impl AsyncMimeDetectorBackend for ConsumerBackend {
    fn content_requirement(&self) -> ContentRequirement {
        if self.0 {
            ContentRequirement::Prefix(1)
        } else {
            ContentRequirement::Complete
        }
    }

    fn random_access_capability(&self) -> RandomAccessCapability {
        if self.0 {
            RandomAccessCapability::SamplesSameObject
        } else {
            RandomAccessCapability::Unsupported
        }
    }

    fn guess_content_in<'a>(
        &'a self,
        _scope: &'a qubit_operation::OperationScope,
        input: BorrowedMimeInput<'a>,
        _options: &'a MimeOperationOptions,
    ) -> MimeFuture<'a, MimeCandidates> {
        let samples = self.0;
        Box::pin(async move {
            let BorrowedMimeInput::ManagedProvider { session, .. } = input else {
                panic!("Complete and SamplesSameObject consumers must receive a managed session")
            };
            let mut data = [0_u8; 8];
            if samples {
                let first = session.read_at(0, &mut data).await?;
                assert_eq!(&data[..first], b"consumer");
                let mut repeated = [0_u8; 3];
                let repeated_count = session.read_at(2, &mut repeated).await?;
                assert_eq!(&repeated[..repeated_count], b"nsu");
            } else {
                let first = session.read(&mut data).await?;
                assert_eq!(&data[..first], b"consumer");
                let eof = session.read(&mut data).await?;
                assert_eq!(eof, 0, "sequential Complete reads probe EOF");
            }
            Ok(MimeCandidates {
                candidates: vec![MimeCandidate {
                    mime_type: "application/x-consumer".into(),
                    rank: 0,
                }],
                inspected_extent: InspectedExtent::Complete,
                retained_prefix: Vec::new(),
                staging_bytes: 0,
            })
        })
    }
}

async fn execute(runtime: &MimeRuntime, path: &str) -> Result<qubit_mime::MimeDetectionReport, MimeError> {
    let mut operation = runtime
        .try_submit_detection(
            OwnedMimeInput::Provider {
                file_system: Arc::new(memory_fs(b"consumer".to_vec(), path.ends_with("fail"))),
                path: qubit_fs::Path::parse(path).unwrap(),
            },
            MimeOperationOptions::default(),
        )
        .unwrap_or_else(|_| panic!("prepared operation is admitted"));
    match operation.wait_outcome().await.unwrap() {
        qubit_operation::OperationOutcome::Succeeded(report) => Ok(report),
        qubit_operation::OperationOutcome::Failed(error) => Err(error),
        other => panic!("unexpected operation outcome: {other:?}"),
    }
}

fn memory_fs(bytes: Vec<u8>, fail_random_read: bool) -> qubit_fs::AsyncFileSystem {
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
            FileSystemId::new("consumer-memory").unwrap(),
            "consumer-memory",
            PathSemantics::Hierarchical,
        ),
        ProviderOperations::new()
            .with(ProviderOperation::OpenReader)
            .with(ProviderOperation::OpenRandomAccessReader),
        FileSystemCapabilities::new()
            .with_guaranteed(FileSystemCapability::Read)
            .with_guaranteed(FileSystemCapability::RangeRead),
        FileSystemLimits::unknown(),
        PathConstraints::absolute(),
        SymlinkPolicy::Reject,
    )
    .unwrap();
    qubit_fs::AsyncFileSystem::from_spi(MemoryFileSystem {
        properties,
        bytes,
        fail_random_read,
    })
    .unwrap()
}

struct MemoryFileSystem {
    properties: qubit_fs::spi::ProviderProperties,
    bytes: Vec<u8>,
    fail_random_read: bool,
}

impl qubit_fs::spi::AsyncFileSystemSpi for MemoryFileSystem {
    fn properties(&self) -> qubit_fs::spi::ProviderProperties {
        self.properties.clone()
    }

    fn stat<'a>(
        &'a self,
        _request: qubit_fs::spi::StatRequest<'a>,
    ) -> qubit_fs::spi::SpiFuture<'a, qubit_fs::FsResult<qubit_fs::spi::StatResponse>> {
        Box::pin(async { panic!("managed consumer reads do not stat input") })
    }

    fn open_reader<'a>(
        &'a self,
        request: qubit_fs::spi::OpenReaderRequest<'a>,
    ) -> qubit_fs::spi::SpiFuture<'a, qubit_fs::FsResult<qubit_fs::spi::OpenedAsyncReader>> {
        let info = qubit_fs::metadata::OpenedFileInfo::new(
            qubit_fs::metadata::FileSystemId::new("consumer-memory").unwrap(),
            request.path().clone(),
        );
        let input = MemoryInput {
            bytes: self.bytes.clone(),
            offset: 0,
        };
        Box::pin(async move { Ok(qubit_fs::spi::OpenedAsyncReader::new(info, Box::new(input))) })
    }

    fn open_random_access_reader<'a>(
        &'a self,
        request: qubit_fs::spi::OpenReaderRequest<'a>,
    ) -> qubit_fs::spi::SpiFuture<'a, qubit_fs::FsResult<qubit_fs::spi::OpenedAsyncRandomAccessReader>> {
        let info = qubit_fs::metadata::OpenedFileInfo::new(
            qubit_fs::metadata::FileSystemId::new("consumer-memory").unwrap(),
            request.path().clone(),
        )
        .with_metadata(
            qubit_fs::metadata::FileMetadata::new(qubit_fs::metadata::FileKind::File)
                .with_len(Some(self.bytes.len() as u64)),
        );
        let bytes = self.bytes.clone();
        let fail_first_read = self.fail_random_read;
        Box::pin(async move {
            Ok(qubit_fs::spi::OpenedAsyncRandomAccessReader::new(
                info,
                Box::new(RandomInput { bytes, fail_first_read }),
            ))
        })
    }
}

struct MemoryInput {
    bytes: Vec<u8>,
    offset: usize,
}
impl qubit_io::AsyncInput for MemoryInput {
    type Item = u8;
    unsafe fn poll_read_unchecked(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        output: &mut [u8],
        index: usize,
        count: usize,
    ) -> Poll<std::io::Result<usize>> {
        let this = self.get_mut();
        let length = count.min(this.bytes.len().saturating_sub(this.offset));
        output[index..index + length].copy_from_slice(&this.bytes[this.offset..this.offset + length]);
        this.offset += length;
        Poll::Ready(Ok(length))
    }
}

struct RandomInput {
    bytes: Vec<u8>,
    fail_first_read: bool,
}
impl qubit_fs::spi::AsyncRandomAccessReadSession for RandomInput {
    fn read_at_async<'a>(
        self: Pin<&'a mut Self>,
        offset: u64,
        output: &'a mut [u8],
    ) -> qubit_fs::spi::SpiFuture<'a, qubit_fs::FsResult<usize>> {
        Box::pin(async move {
            let this = self.get_mut();
            if this.fail_first_read {
                this.fail_first_read = false;
                output.fill(b'!');
                return Err(qubit_fs::FsError::new(
                    qubit_fs::error::FsErrorKind::Io,
                    qubit_fs::error::FsOperation::Read,
                    "consumer injected read failure",
                ));
            }
            let start = usize::try_from(offset).unwrap_or(usize::MAX);
            let length = output.len().min(this.bytes.len().saturating_sub(start));
            output[..length].copy_from_slice(&this.bytes[start..start + length]);
            Ok(length)
        })
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
    let mut future = Box::pin(future);
    loop {
        match Pin::as_mut(&mut future).poll(&mut context) {
            Poll::Ready(result) => return result,
            Poll::Pending => std::thread::park(),
        }
    }
}

fn main() {
    let manager = OperationManager::new(Default::default()).expect("manager");
    let group = manager
        .group(qubit_operation::GroupOptions { capacity: 8 })
        .expect("MIME group");
    let services =
        MimeExecutionServices::new(manager, group, Arc::new(FixedMimeExecutionPort::new(1, 8)), 16).expect("services");
    services.prepare().expect("prepare services");
    let registry = MimeDetectorRegistry::default();
    registry
        .register_async_provider(ConsumerProvider(false))
        .expect("register Complete provider");
    registry
        .register_async_provider(ConsumerProvider(true))
        .expect("register SamplesSameObject provider");
    let runtime = MimeRuntime::builtin()
        .expect("runtime")
        .with_execution_services(services.clone())
        .with_async_provider_selection(&registry, &qubit_spi::ProviderSelection::named("complete").unwrap())
        .expect("select Complete consumer");
    let report = block_on(execute(&runtime, "/consumer")).expect("Complete managed read");
    assert_eq!(report.mime_type.as_deref(), Some("application/x-consumer"));
    assert_eq!(report.budget.bytes_read, 8);
    let samples_runtime = MimeRuntime::builtin()
        .expect("runtime")
        .with_execution_services(services.clone())
        .with_async_provider_selection(&registry, &qubit_spi::ProviderSelection::named("samples").unwrap())
        .expect("select sampling consumer");
    let samples = block_on(execute(&samples_runtime, "/consumer-samples")).expect("same-object samples");
    assert_eq!(
        samples.budget.bytes_read, 11,
        "repeated random samples count cumulatively"
    );
    assert!(
        block_on(execute(&samples_runtime, "/controlled-fail")).is_err(),
        "injected random read failures reach the consumer"
    );
    let parent_manager = OperationManager::new(Default::default()).expect("parent manager");
    let parent_group = parent_manager
        .group(qubit_operation::GroupOptions { capacity: 4 })
        .expect("parent MIME group");
    let parent_services = MimeExecutionServices::new(
        parent_manager.clone(),
        parent_group,
        Arc::new(FixedMimeExecutionPort::new(1, 4)),
        16,
    )
    .expect("parent services");
    parent_services.prepare().expect("prepare parent services");
    let parent_runtime = MimeRuntime::builtin()
        .expect("runtime")
        .with_execution_services(parent_services.clone())
        .with_async_provider_selection(&registry, &qubit_spi::ProviderSelection::named("complete").unwrap())
        .expect("select Complete consumer for parent siblings");
    let parent_options = qubit_operation::OperationOptions {
        child_failure_policy: qubit_operation::ChildFailurePolicy::ContinueSiblings,
        limits: qubit_operation::ScopeLimits {
            child_operations: 2,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut parent = parent_manager
        .as_async()
        .try_submit(parent_options, move |scope| async move {
            let child_options = MimeOperationOptions {
                max_buffer_bytes: Some(8),
                max_read_bytes: Some(16),
                operation: qubit_operation::OperationOptions {
                    requirement: qubit_operation::ChildRequirement::Optional,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut child_results = Vec::new();
            for child_path in ["/parent-sibling-a", "/parent-sibling-b"] {
                let mut child = parent_runtime
                    .try_submit_detection_in(
                        &scope,
                        OwnedMimeInput::Provider {
                            file_system: Arc::new(memory_fs(b"consumer".to_vec(), false)),
                            path: qubit_fs::Path::parse(child_path).unwrap(),
                        },
                        child_options.clone(),
                    )
                    .unwrap_or_else(|_| panic!("parent admits both managed children"));
                let outcome = child.wait_outcome().await.unwrap();
                let _ = child.wait_settlement().await;
                child_results.push(outcome);
            }
            Ok::<_, ()>(child_results)
        })
        .expect("parent operation is admitted");
    let sibling_outcomes = block_on(parent.wait_outcome()).unwrap();
    let qubit_operation::OperationOutcome::Succeeded(sibling_outcomes) = sibling_outcomes else {
        panic!("optional sibling read failure should not fail the parent: {sibling_outcomes:?}");
    };
    block_on(parent.wait_settlement());
    assert_eq!(sibling_outcomes.len(), 2);
    match &sibling_outcomes[0] {
        qubit_operation::OperationOutcome::Succeeded(report) => {
            assert_eq!(report.budget.bytes_read, 8, "first sibling consumes the shared budget");
        }
        other => panic!("first sibling should use the parent budget successfully: {other:?}"),
    }
    assert!(
        matches!(
            &sibling_outcomes[1],
            qubit_operation::OperationOutcome::Failed(MimeError::BudgetExceeded { kind: "read", .. })
        ),
        "second sibling must observe the first sibling's cumulative parent read: {:?}",
        sibling_outcomes[1]
    );
    let parent_shutdown = parent_services.shutdown();
    assert!(block_on(parent_shutdown.wait_services_settled()).is_ok());
    assert!(parent_shutdown.stop_services().is_ok());
    let shutdown = services.shutdown();
    assert!(block_on(shutdown.wait_services_settled()).is_ok());
    assert!(shutdown.stop_services().is_ok());
}
