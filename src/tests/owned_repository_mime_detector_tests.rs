// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

//! Tests crate-private repository detector contracts through supported traits.

use std::future::Future;
use std::io::Cursor;
use std::sync::Arc;
use std::task::Context;
use std::task::Poll;
use std::task::Wake;
use std::task::Waker;

use qubit_fs_local::LocalFileSystems;
use qubit_fs_local::LocalResourcePolicy;
use qubit_fs_local::host_path_to_logical;

use crate::AsyncMimeDetectorBackend;
use crate::ContentRequirement;
use crate::MimeConfig;
use crate::MimeDetectionPolicy;
use crate::MimeDetector;
use crate::MimeDetectorBackend;
use crate::MimeRepository;
use crate::RandomAccessCapability;
use crate::StreamBasedMimeDetector;
use crate::detector::owned_repository_mime_detector::OwnedRepositoryMimeDetector;

/// Unparks the thread polling a future when that future is woken.
struct ThreadWake(std::thread::Thread);

impl Wake for ThreadWake {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}

/// Polls a future on the current thread, parking until it is woken.
fn block_on<F: Future>(future: F) -> F::Output {
    let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(value) => return value,
            Poll::Pending => std::thread::park(),
        }
    }
}

#[test]
fn test_owned_repository_detector_exercises_stream_and_backend_entry_points() {
    let detector = OwnedRepositoryMimeDetector::new(MimeConfig::default(), MimeRepository::bundled_shared());
    let bytes = b"%PDF-1.7\n";
    assert!(matches!(
        MimeDetectorBackend::content_requirement(&detector),
        ContentRequirement::Prefix(n) if n > 0
    ));
    assert!(matches!(
        MimeDetector::content_requirement(&detector),
        ContentRequirement::Prefix(n) if n > 0
    ));
    assert_eq!(
        RandomAccessCapability::Unsupported,
        AsyncMimeDetectorBackend::random_access_capability(&detector)
    );
    assert_eq!(
        vec!["application/pdf"],
        StreamBasedMimeDetector::guess_from_content_bytes(&detector, bytes)
            .expect("repository should identify the PDF signature")
    );
    assert_eq!(
        vec!["application/pdf"],
        MimeDetectorBackend::guess_from_content(&detector, bytes)
            .expect("backend delegation should identify the PDF signature")
    );
    assert_eq!(
        vec!["application/pdf"],
        StreamBasedMimeDetector::guess_from_filename(&detector, "report.pdf")
    );
    assert_eq!(
        Some("application/pdf".to_owned()),
        detector
            .detect_by_filename("report.pdf")
            .expect("filename lookup should succeed")
    );
    assert_eq!(
        Some("application/pdf".to_owned()),
        detector
            .detect_by_content(bytes)
            .expect("content lookup should succeed")
    );
    assert_eq!(
        Some("application/pdf".to_owned()),
        detector
            .detect(bytes, Some("report.pdf"), MimeDetectionPolicy::VerifyContent)
            .expect("combined lookup should verify the PDF content")
    );
    assert_eq!(
        Some("application/pdf".to_owned()),
        detector
            .detect_prefix(bytes, Some("report.pdf"), MimeDetectionPolicy::VerifyContent)
            .expect("bounded prefix lookup should verify the PDF content")
    );

    let (reader_candidates, reader_prefix) =
        MimeDetectorBackend::guess_from_reader(&detector, &mut Cursor::new(bytes.to_vec()))
            .expect("default backend reader path should use the repository prefix");
    assert_eq!(vec!["application/pdf"], reader_candidates);
    assert_eq!(bytes, reader_prefix.as_slice());
    let reader_result = detector
        .detect_reader(
            &mut Cursor::new(bytes.to_vec()),
            Some("report.pdf"),
            MimeDetectionPolicy::VerifyContent,
        )
        .expect("top-level reader path should use the owned repository");
    assert_eq!(Some("application/pdf".to_owned()), reader_result);

    let path = std::env::temp_dir().join(format!("owned-repository-backend-{}.pdf", std::process::id()));
    std::fs::write(&path, bytes).expect("test PDF should be written");
    let (file_candidates, file_prefix) = MimeDetectorBackend::guess_from_file(&detector, &path)
        .expect("default backend file path should read the repository prefix");
    assert_eq!(vec!["application/pdf"], file_candidates);
    assert_eq!(bytes, file_prefix.as_slice());
    assert_eq!(
        Some("application/pdf".to_owned()),
        detector
            .detect_file(&path, MimeDetectionPolicy::VerifyContent)
            .expect("top-level file path should use the owned repository")
    );

    let filesystem = LocalFileSystems::host(LocalResourcePolicy::unbounded()).unwrap();
    let logical_path = host_path_to_logical(&path).expect("host path should convert losslessly");
    let (path_candidates, content) = MimeDetectorBackend::guess_from_provider_path(
        &detector,
        &filesystem,
        &logical_path,
        detector.max_buffer_size(),
    )
    .expect("default provider path should read a bounded prefix");
    assert_eq!(vec!["application/pdf"], path_candidates);
    assert_eq!(Some(bytes.to_vec()), content);
    assert_eq!(
        Some("application/pdf".to_owned()),
        detector
            .detect_path(
                &filesystem,
                &logical_path,
                detector.max_buffer_size(),
                MimeDetectionPolicy::VerifyContent,
            )
            .expect("top-level provider path should use the owned repository")
    );
    std::fs::remove_file(path).unwrap();

    assert!(matches!(
        block_on(MimeDetectorBackend::guess_from_content_async(&detector, bytes)),
        Err(crate::MimeError::AsyncUnsupported)
    ));
}
