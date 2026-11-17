// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Checks file-based classification, staged content, and staging limits.

use std::path::Path;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use qubit_mime::FileBasedMediaStreamClassifier;
use qubit_mime::MediaStreamClassifier;
use qubit_mime::MediaStreamType;
use qubit_mime::MimeResult;

/// Classifies existing files as video with audio and missing paths as none.
#[derive(Debug)]
struct FileOnlyClassifier;

impl FileBasedMediaStreamClassifier for FileOnlyClassifier {
    fn classify_by_local_file(&self, file: &Path) -> MimeResult<MediaStreamType> {
        Ok(if file.is_file() {
            MediaStreamType::VideoWithAudio
        } else {
            MediaStreamType::None
        })
    }
}

#[test]
fn test_file_based_media_stream_classifier_stages_content_to_temp_file() {
    let classifier = FileOnlyClassifier;

    assert_eq!(
        MediaStreamType::VideoWithAudio,
        classifier
            .classify_content(b"media")
            .expect("staged content should be classified")
    );
    assert_eq!(
        MediaStreamType::VideoWithAudio,
        classifier
            .classify_file(Path::new("Cargo.toml"))
            .expect("existing file should be classified")
    );
}

/// Enforces a three-byte staging limit and counts file-classification calls.
#[derive(Debug, Default)]
struct LimitedFileOnlyClassifier {
    calls: AtomicUsize,
}

impl FileBasedMediaStreamClassifier for LimitedFileOnlyClassifier {
    #[inline]
    fn max_staging_size(&self) -> u64 {
        3
    }

    fn classify_by_local_file(&self, file: &Path) -> MimeResult<MediaStreamType> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        assert!(file.is_file());
        Ok(MediaStreamType::AudioOnly)
    }
}

#[test]
fn test_file_based_classifier_enforces_its_public_staging_limit() {
    let classifier = LimitedFileOnlyClassifier::default();

    assert!(matches!(
        classifier.classify_content(b"four"),
        Err(qubit_mime::MimeError::InvalidClassifierInput { .. })
    ));
    assert_eq!(0, classifier.calls.load(Ordering::SeqCst));
    assert_eq!(MediaStreamType::AudioOnly, classifier.classify_content(b"abc").unwrap());
    assert_eq!(1, classifier.calls.load(Ordering::SeqCst));
}
