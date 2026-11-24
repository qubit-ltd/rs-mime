// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::future::Future;
use std::io::Read;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::task::Context;
use std::task::Poll;
use std::task::Wake;
use std::task::Waker;

use qubit_mime::AsyncMediaStreamClassifierBackend;
use qubit_mime::AsyncMediaStreamClassifierProvider;
use qubit_mime::BorrowedMimeInput;
use qubit_mime::CONFIG_MIME_ENABLE_PRECISE_DETECTION;
use qubit_mime::FfprobeCommandMediaStreamClassifierProvider;
use qubit_mime::FixedMimeExecutionPort;
use qubit_mime::InputExtent;
use qubit_mime::MediaStreamClassifier;
use qubit_mime::MediaStreamType;
use qubit_mime::MimeConfig;
use qubit_mime::MimeDetectionPolicy;
use qubit_mime::MimeDetectorCore;
use qubit_mime::MimeExecutionServices;
use qubit_mime::MimeFuture;
use qubit_mime::MimeOperationOptions;
use qubit_mime::MimeResult;
use qubit_operation::OperationOutcome;
use qubit_operation::OperationScope;

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
struct PanicSyncClassifier;
impl MediaStreamClassifier for PanicSyncClassifier {
    fn classify_file(&self, _file: &Path) -> MimeResult<MediaStreamType> {
        panic!("sync classifier called")
    }
    fn classify_reader(&self, _reader: &mut dyn Read) -> MimeResult<MediaStreamType> {
        panic!("sync classifier called")
    }
}

#[derive(Debug)]
struct AsyncClassifier {
    calls: AtomicUsize,
    result: MediaStreamType,
}
impl AsyncMediaStreamClassifierBackend for AsyncClassifier {
    fn classify_in<'a>(
        &'a self,
        _scope: &'a OperationScope,
        _input: BorrowedMimeInput<'a>,
        _options: &'a MimeOperationOptions,
    ) -> MimeFuture<'a, MediaStreamType> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let result = self.result;
        Box::pin(async move { Ok(result) })
    }
}

fn audio_classifier() -> Arc<AsyncClassifier> {
    Arc::new(AsyncClassifier {
        calls: AtomicUsize::new(0),
        result: MediaStreamType::AudioOnly,
    })
}

fn refinement_config() -> MimeConfig {
    let mut config = qubit_config::Config::new();
    config.set(CONFIG_MIME_ENABLE_PRECISE_DETECTION, "true").unwrap();
    MimeConfig::from_config(&config).unwrap()
}

fn complete_bytes<'a>() -> BorrowedMimeInput<'a> {
    BorrowedMimeInput::Bytes {
        content: b"ogg",
        extent: InputExtent::Complete,
        filename: None,
    }
}

#[test]
fn test_refinement_operation_async_path_never_calls_sync_classifier() {
    let mut core = MimeDetectorCore::new(refinement_config());
    core.set_media_stream_classifier(Some(Arc::new(PanicSyncClassifier)));
    let async_classifier = audio_classifier();
    core.set_async_media_stream_classifier(Some(async_classifier.clone()));
    let core = Arc::new(core);
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let filename = vec!["video/ogg".to_owned()];
    let content = vec!["video/ogg".to_owned()];
    let options = MimeOperationOptions {
        policy: MimeDetectionPolicy::ContentFirst,
        ..Default::default()
    };
    let mut operation = manager
        .as_async()
        .try_submit(Default::default(), move |scope| {
            let core = Arc::clone(&core);
            async move {
                core.select_result_in(&scope, &filename, &content, complete_bytes(), &options)
                    .await
            }
        })
        .unwrap();
    match block_on(operation.wait_outcome()).unwrap() {
        OperationOutcome::Succeeded(Some(value)) => assert_eq!(value, "audio/ogg"),
        other => panic!("unexpected operation outcome: {other:?}"),
    }
    assert_eq!(async_classifier.calls.load(Ordering::SeqCst), 1);
}

#[test]
fn test_refinement_operation_prefer_filename_fast_path_skips_classifier() {
    let mut core = MimeDetectorCore::new(MimeConfig::default());
    let async_classifier = audio_classifier();
    core.set_async_media_stream_classifier(Some(async_classifier.clone()));
    let core = Arc::new(core);
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let filename = vec!["video/ogg".to_owned()];
    let content = vec!["audio/ogg".to_owned()];
    let options = MimeOperationOptions {
        policy: MimeDetectionPolicy::PreferFilename,
        ..Default::default()
    };
    let mut operation = manager
        .as_async()
        .try_submit(Default::default(), move |scope| {
            let core = Arc::clone(&core);
            async move {
                core.select_result_in(&scope, &filename, &content, complete_bytes(), &options)
                    .await
            }
        })
        .unwrap();
    match block_on(operation.wait_outcome()).unwrap() {
        OperationOutcome::Succeeded(Some(value)) => assert_eq!(value, "video/ogg"),
        other => panic!("unexpected operation outcome: {other:?}"),
    }
    assert_eq!(async_classifier.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn test_refinement_operation_without_classifier_does_not_launch_work() {
    let core = Arc::new(MimeDetectorCore::new(MimeConfig::default()));
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let filename = vec!["video/ogg".to_owned()];
    let content = vec!["video/ogg".to_owned()];
    let options = MimeOperationOptions {
        policy: MimeDetectionPolicy::ContentFirst,
        ..Default::default()
    };
    let mut operation = manager
        .as_async()
        .try_submit(Default::default(), move |scope| {
            let core = Arc::clone(&core);
            async move {
                core.select_result_in(&scope, &filename, &content, complete_bytes(), &options)
                    .await
            }
        })
        .unwrap();
    match block_on(operation.wait_outcome()).unwrap() {
        OperationOutcome::Succeeded(Some(value)) => assert_eq!(value, "video/ogg"),
        other => panic!("unexpected operation outcome: {other:?}"),
    }
}

#[cfg(unix)]
#[test]
fn test_public_ffprobe_async_provider_classifies_complete_bytes() {
    use std::os::unix::fs::PermissionsExt;

    use qubit_local_files::LocalFileSystem;
    use qubit_local_files::options::LocalTempDirectoryOptions;

    use crate::support::PathEnvGuard;

    let directory = LocalFileSystem::host()
        .unwrap()
        .create_temp_directory_with_options(&LocalTempDirectoryOptions::new())
        .unwrap();
    let executable = directory.path().join("ffprobe");
    std::fs::write(&executable, "#!/bin/sh\nprintf 'video\\naudio\\n'\n").unwrap();
    let mut permissions = std::fs::metadata(&executable).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&executable, permissions).unwrap();
    let _path_guard = PathEnvGuard::prepend(directory.path());

    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let group = manager.group(qubit_operation::GroupOptions { capacity: 1 }).unwrap();
    let services = Arc::new(
        MimeExecutionServices::new(
            manager.clone(),
            group.clone(),
            Arc::new(FixedMimeExecutionPort::new(1, 2)),
            64,
        )
        .unwrap(),
    );
    services.prepare().unwrap();
    let provider = FfprobeCommandMediaStreamClassifierProvider;
    let backend = provider
        .create_async_backend(&MimeConfig::default(), &services)
        .unwrap();
    let incomplete_backend = Arc::clone(&backend);
    let mut incomplete = group
        .as_async()
        .try_submit(Default::default(), move |scope| async move {
            incomplete_backend
                .classify_in(
                    &scope,
                    BorrowedMimeInput::Bytes {
                        content: b"partial",
                        extent: InputExtent::Prefix,
                        filename: None,
                    },
                    &MimeOperationOptions::default(),
                )
                .await
        })
        .unwrap();
    assert!(matches!(
        block_on(incomplete.wait_outcome()).unwrap(),
        OperationOutcome::Failed(qubit_mime::MimeError::CompleteContentRequired)
    ));
    assert!(block_on(incomplete.wait_settlement()).all_released());

    let zero_limit_backend = Arc::clone(&backend);
    let mut zero_limit = group
        .as_async()
        .try_submit(Default::default(), move |scope| async move {
            zero_limit_backend
                .classify_in(
                    &scope,
                    BorrowedMimeInput::Bytes {
                        content: b"complete",
                        extent: InputExtent::Complete,
                        filename: None,
                    },
                    &MimeOperationOptions {
                        max_staging_bytes: Some(0),
                        ..Default::default()
                    },
                )
                .await
        })
        .unwrap();
    assert!(matches!(
        block_on(zero_limit.wait_outcome()).unwrap(),
        OperationOutcome::Failed(qubit_mime::MimeError::InvalidOperationBudget)
    ));
    assert!(block_on(zero_limit.wait_settlement()).all_released());

    let mut operation = group
        .as_async()
        .try_submit(Default::default(), move |scope| async move {
            let options = MimeOperationOptions::default();
            backend
                .classify_in(
                    &scope,
                    BorrowedMimeInput::Bytes {
                        content: b"complete media",
                        extent: InputExtent::Complete,
                        filename: None,
                    },
                    &options,
                )
                .await
        })
        .unwrap();

    assert!(matches!(
        block_on(operation.wait_outcome()).unwrap(),
        OperationOutcome::Succeeded(MediaStreamType::VideoWithAudio)
    ));
    assert!(block_on(operation.wait_settlement()).all_released());
    let shutdown = services.shutdown();
    let _ = block_on(shutdown.wait_services_settled()).unwrap();
    let _ = shutdown.stop_services().unwrap();
    let _report = block_on(shutdown.wait_group_shutdown()).unwrap();
}
