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
use std::sync::atomic::Ordering;
use std::task::Context;
use std::task::Poll;
use std::task::Wake;
use std::task::Waker;

use qubit_fs::AsyncFileSystem;
use qubit_fs::Path as FsPath;
use qubit_mime::ContentRequirement;
use qubit_mime::FileCommandMimeDetector;
use qubit_mime::MimeDetectionPolicy;
use qubit_mime::MimeDetector;
use qubit_mime::MimeDetectorBackend;
use qubit_mime::MimeDetectorCore;
use qubit_mime::MimeError;
use qubit_mime::MimeResult;

use super::mime_detector_tests::PrefixAsyncFileSystemSpi;

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
        match Future::poll(Pin::as_mut(&mut future), &mut cx) {
            Poll::Ready(result) => return result,
            Poll::Pending => std::thread::park(),
        }
    }
}

#[test]
fn file_legacy_async_content_rejects_before_provider_io() {
    let detector = FileCommandMimeDetector::new();
    assert!(!MimeDetectorBackend::supports_async_content_detection(
        &detector
    ));
    let path = FsPath::parse("/unknown.bin").expect("fixture path");

    for spi in [
        PrefixAsyncFileSystemSpi::new(b"payload"),
        PrefixAsyncFileSystemSpi::new(b"").failing_open(),
    ] {
        let opens = Arc::clone(&spi.opens);
        let reads = Arc::clone(&spi.reads);
        let fs = AsyncFileSystem::from_spi(spi).expect("fixture filesystem");
        let error = block_on(detector.detect_async_path(
            &fs,
            &path,
            32,
            MimeDetectionPolicy::VerifyContent,
        ))
        .expect_err("file detector has no legacy async content hook");
        assert!(matches!(error, MimeError::AsyncUnsupported));
        assert_eq!(0, opens.load(Ordering::Relaxed));
        assert_eq!(0, reads.load(Ordering::Relaxed));
    }
}

#[test]
fn file_legacy_async_filename_shortcut_and_default_provider_hook_capability() {
    let detector = FileCommandMimeDetector::new();
    let path = FsPath::parse("/document.pdf").expect("PDF path");
    let spi = PrefixAsyncFileSystemSpi::new(b"%PDF-1.7");
    let opens = Arc::clone(&spi.opens);
    let reads = Arc::clone(&spi.reads);
    let fs = AsyncFileSystem::from_spi(spi).expect("fixture filesystem");
    assert_eq!(
        Some("application/pdf".to_owned()),
        block_on(detector.detect_async_path(&fs, &path, 32, MimeDetectionPolicy::PreferFilename))
            .expect("unique filename should short-circuit"),
    );
    assert_eq!(0, opens.load(Ordering::Relaxed));
    assert_eq!(0, reads.load(Ordering::Relaxed));

    let error = block_on(MimeDetectorBackend::guess_from_async_provider_path(
        &detector, &fs, &path, 32,
    ))
    .expect_err("default low-level hook should respect the capability");
    assert!(matches!(error, MimeError::AsyncUnsupported));
    assert_eq!(0, opens.load(Ordering::Relaxed));
    assert_eq!(0, reads.load(Ordering::Relaxed));
}

#[derive(Debug)]
struct CompleteBackend;
impl MimeDetectorBackend for CompleteBackend {
    fn content_requirement(&self) -> ContentRequirement {
        ContentRequirement::Complete
    }
    fn core(&self) -> &MimeDetectorCore {
        static CORE: std::sync::OnceLock<MimeDetectorCore> = std::sync::OnceLock::new();
        CORE.get_or_init(MimeDetectorCore::default)
    }
    fn max_test_bytes(&self) -> usize {
        8
    }
    fn guess_from_filename(&self, _filename: &str) -> Vec<String> {
        Vec::new()
    }
    fn guess_from_content(&self, _content: &[u8]) -> MimeResult<Vec<String>> {
        Ok(Vec::new())
    }
}

#[test]
fn default_provider_hook_preserves_complete_content_error_before_io() {
    let path = FsPath::parse("/unknown.bin").expect("fixture path");
    let spi = PrefixAsyncFileSystemSpi::new(b"payload");
    let opens = Arc::clone(&spi.opens);
    let reads = Arc::clone(&spi.reads);
    let fs = AsyncFileSystem::from_spi(spi).expect("fixture filesystem");
    let error = block_on(MimeDetectorBackend::guess_from_async_provider_path(
        &CompleteBackend,
        &fs,
        &path,
        32,
    ))
    .expect_err("complete input is rejected by the default bounded hook");
    assert!(matches!(error, MimeError::CompleteContentRequired));
    assert_eq!(0, opens.load(Ordering::Relaxed));
    assert_eq!(0, reads.load(Ordering::Relaxed));
}
