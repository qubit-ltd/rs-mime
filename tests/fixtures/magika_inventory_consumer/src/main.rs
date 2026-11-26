// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::error::Error;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::task::Context;
use std::task::Poll;
use std::time::Duration;

use qubit_fs::AsyncFileSystem;
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
#[cfg(not(feature = "inventory"))]
use qubit_magika::MagikaMimeDetectorProvider;
use qubit_magika::ManagedInferenceRuntime;
use qubit_mime::MimeConfig;
use qubit_mime::MimeDetectionPolicy;
use qubit_mime::MimeDetector;
use qubit_mime::MimeDetectorRegistry;
use qubit_spi::ProviderSelection;
use qubit_spi::ProviderSelectionTargetRef;

const PYTHON: &[u8] = b"#!/usr/bin/env python3\nprint('hello')\n";
const ASYNC_PATH: &str = "/fixture/not-python.txt";
const WAIT_LIMIT: Duration = Duration::from_secs(30);

#[derive(Clone)]
struct ProviderIo {
    opens: Arc<AtomicUsize>,
    read_polls: Arc<AtomicUsize>,
    bytes_read: Arc<AtomicUsize>,
}

struct FixtureFileSystem {
    properties: ProviderProperties,
    io: ProviderIo,
}

impl FixtureFileSystem {
    fn new() -> Result<(AsyncFileSystem, ProviderIo), Box<dyn Error>> {
        let properties = ProviderProperties::new(
            FileSystemInfo::new(
                FileSystemId::new("magika-consumer-fixture")?,
                "magika-consumer-fixture",
                PathSemantics::Hierarchical,
            ),
            ProviderOperations::new().with(ProviderOperation::OpenReader),
            FileSystemCapabilities::new().with_guaranteed(FileSystemCapability::Read),
            FileSystemLimits::unknown(),
            PathConstraints::absolute(),
            SymlinkPolicy::Reject,
        )?;
        let io = ProviderIo {
            opens: Arc::new(AtomicUsize::new(0)),
            read_polls: Arc::new(AtomicUsize::new(0)),
            bytes_read: Arc::new(AtomicUsize::new(0)),
        };
        let filesystem = AsyncFileSystem::from_spi(Self {
            properties,
            io: io.clone(),
        })?;
        Ok((filesystem, io))
    }
}

impl AsyncFileSystemSpi for FixtureFileSystem {
    fn properties(&self) -> ProviderProperties {
        self.properties.clone()
    }

    fn stat<'a>(&'a self, _request: StatRequest<'a>) -> SpiFuture<'a, FsResult<StatResponse>> {
        Box::pin(async {
            Err(qubit_fs::FsError::new(
                FsErrorKind::UnsupportedOperation,
                FsOperation::Stat,
                "fixture deliberately does not support stat",
            ))
        })
    }

    fn open_reader<'a>(&'a self, request: OpenReaderRequest<'a>) -> SpiFuture<'a, FsResult<OpenedAsyncReader>> {
        let info = OpenedFileInfo::new(
            FileSystemId::new("magika-consumer-fixture").expect("valid fixture id"),
            request.path().clone(),
        );
        self.io.opens.fetch_add(1, Ordering::SeqCst);
        Box::pin(async {
            Ok(OpenedAsyncReader::new(
                info,
                Box::new(PythonInput {
                    at: 0,
                    io: self.io.clone(),
                }),
            ))
        })
    }
}

struct PythonInput {
    at: usize,
    io: ProviderIo,
}

impl AsyncInput for PythonInput {
    type Item = u8;

    unsafe fn poll_read_unchecked(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        output: &mut [u8],
        index: usize,
        count: usize,
    ) -> Poll<std::io::Result<usize>> {
        self.io.read_polls.fetch_add(1, Ordering::SeqCst);
        let amount = count.min(PYTHON.len().saturating_sub(self.at));
        if amount > 0 {
            output[index..index + amount].copy_from_slice(&PYTHON[self.at..self.at + amount]);
            self.at += amount;
            self.io.bytes_read.fetch_add(amount, Ordering::SeqCst);
        }
        Poll::Ready(Ok(amount))
    }
}

fn create_detector() -> Result<
    (
        MimeDetectorRegistry,
        Arc<dyn MimeDetector>,
        Option<Arc<ManagedInferenceRuntime>>,
    ),
    Box<dyn Error>,
> {
    let config = MimeConfig::default();
    let registry = MimeDetectorRegistry::builtin();
    assert!(matches!(
        registry.default_selection().target(),
        ProviderSelectionTargetRef::Named(selector) if selector.as_str() == "repository"
    ));

    #[cfg(feature = "inventory")]
    {
        assert!(registry.provider_ids().iter().any(|id| id.as_str() == "magika"));
        let selection = ProviderSelection::named("magika")?;
        let detector = registry.resolve_selected(&selection)?.create_configured(&config)?;
        Ok((registry, detector, None))
    }

    #[cfg(not(feature = "inventory"))]
    {
        let manager = qubit_operation::OperationManager::new(qubit_operation::ManagerOptions::default())?;
        let runtime = Arc::new(ManagedInferenceRuntime::new(manager, config.max_buffer_size())?);
        registry.register(MagikaMimeDetectorProvider::new().managed_runtime(Arc::clone(&runtime)))?;
        let selection = ProviderSelection::named("magika")?;
        let detector = registry.resolve_selected(&selection)?.create_configured(&config)?;
        Ok((registry, detector, Some(runtime)))
    }
}

fn detect_async_on_operation_manager(
    detector: Arc<dyn MimeDetector>,
    filesystem: AsyncFileSystem,
) -> Result<Option<String>, Box<dyn Error>> {
    let manager = qubit_operation::OperationManager::new(qubit_operation::ManagerOptions::default())?;
    let group = manager.group(qubit_operation::GroupOptions { capacity: 1 })?;
    let operation = group
        .as_async()
        .try_submit(qubit_operation::OperationOptions::default(), move |_| async move {
            detector
                .detect_async_path(
                    &filesystem,
                    &Path::parse(ASYNC_PATH).expect("fixture path is valid"),
                    4096,
                    MimeDetectionPolicy::VerifyContent,
                )
                .await
        })?;

    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut operation = operation.into_sync();
        let result = operation
            .wait_outcome()
            .map_err(|error| format!("async operation outcome: {error}"))
            .and_then(|outcome| match outcome {
                qubit_operation::OperationOutcome::Succeeded(value) => Ok(value),
                other => Err(format!("async operation failed: {other:?}")),
            })
            .and_then(|value| {
                operation
                    .wait_settlement()
                    .map_err(|error| format!("async operation settlement: {error}"))
                    .and_then(|settlement| {
                        if settlement.all_released() {
                            Ok(value)
                        } else {
                            Err(format!("async operation resources not released: {settlement:?}"))
                        }
                    })
            });
        let _ = tx.send(result);
    });
    match rx.recv_timeout(WAIT_LIMIT) {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => Err(Box::<dyn Error>::from(error)),
        Err(error) => Err(format!("async detect operation exceeded {WAIT_LIMIT:?}: {error}").into()),
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let library = std::env::var_os("ORT_LIBRARY_PATH")
        .map(std::path::PathBuf::from)
        .ok_or("ORT_LIBRARY_PATH must name the ONNX Runtime shared library")?;
    ort::init_from(library)?.commit();

    let (registry, detector, runtime) = create_detector()?;
    assert_eq!(
        Some("text/x-python".to_owned()),
        detector.detect(
            b"#!/usr/bin/env python3\nprint('hello')\n",
            Some("upload.txt"),
            MimeDetectionPolicy::VerifyContent,
        )?,
    );
    assert_eq!(
        Some("application/pdf".to_owned()),
        detector.detect_by_filename("document.pdf")?,
    );

    let (filesystem, io) = FixtureFileSystem::new()?;
    let result = detect_async_on_operation_manager(Arc::clone(&detector), filesystem)?;
    assert_eq!(
        Some("text/x-python".to_owned()),
        result,
        "content must override the mismatching .txt filename"
    );
    let opens = io.opens.load(Ordering::SeqCst);
    let read_polls = io.read_polls.load(Ordering::SeqCst);
    let bytes_read = io.bytes_read.load(Ordering::SeqCst);
    assert!(opens > 0, "async Provider must open the file; opens={opens}");
    assert!(
        read_polls > 0,
        "async Provider must be polled for reads; reads={read_polls}"
    );
    assert_eq!(
        PYTHON.len(),
        bytes_read,
        "bounded read_all should consume the fixture payload"
    );
    println!(
        "async provider evidence: opens={opens} read_polls={read_polls} bytes_read={bytes_read}/{} result={result:?} path={ASYNC_PATH} limit=4096",
        PYTHON.len()
    );

    drop(detector);
    if let Some(runtime) = runtime {
        let report = runtime.shutdown_blocking(qubit_operation::ShutdownMode::Drain, Duration::from_secs(5));
        assert!(report.worker_stopped);
    }
    drop(registry);
    Ok(())
}
