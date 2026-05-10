/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/

use std::io::{
    Cursor,
    Error,
    Read,
    Result as IoResult,
};
use std::path::Path;
use std::sync::Arc;

use qubit_mime::{
    FileBasedMediaStreamClassifier,
    MediaStreamClassifier,
    MediaStreamClassifierBackend,
    MediaStreamType,
    MimeError,
    MimeResult,
};

#[derive(Debug)]
struct StaticClassifier {
    stream_type: MediaStreamType,
}

impl MediaStreamClassifier for StaticClassifier {
    fn classify_file(&self, _file: &Path) -> MimeResult<MediaStreamType> {
        Ok(self.stream_type)
    }

    fn classify_reader(&self, _reader: &mut dyn Read) -> MimeResult<MediaStreamType> {
        Ok(self.stream_type)
    }
}

#[derive(Debug)]
struct BackendClassifier;

impl MediaStreamClassifierBackend for BackendClassifier {
    fn classify_by_local_file(&self, _file: &Path) -> MimeResult<MediaStreamType> {
        Ok(MediaStreamType::VideoOnly)
    }

    fn classify_by_content(&self, reader: &mut dyn Read) -> MimeResult<MediaStreamType> {
        let mut content = Vec::new();
        reader.read_to_end(&mut content)?;
        if content == b"audio" {
            Ok(MediaStreamType::AudioOnly)
        } else {
            Ok(MediaStreamType::None)
        }
    }
}

#[derive(Debug)]
struct LocalFileOnlyClassifier;

impl FileBasedMediaStreamClassifier for LocalFileOnlyClassifier {
    fn classify_by_local_file(&self, file: &Path) -> MimeResult<MediaStreamType> {
        if file.is_file() {
            Ok(MediaStreamType::VideoWithAudio)
        } else {
            Ok(MediaStreamType::None)
        }
    }
}

#[derive(Debug)]
struct FailingLocalFileOnlyClassifier;

impl FileBasedMediaStreamClassifier for FailingLocalFileOnlyClassifier {
    fn classify_by_local_file(&self, _file: &Path) -> MimeResult<MediaStreamType> {
        Err(MimeError::InvalidClassifierInput {
            reason: "forced".to_owned(),
        })
    }
}

struct ErrorReader;

impl Read for ErrorReader {
    fn read(&mut self, _buf: &mut [u8]) -> IoResult<usize> {
        Err(Error::other("forced read error"))
    }
}

#[test]
fn test_media_stream_classifier_trait_supports_content_classification() {
    let classifier = StaticClassifier {
        stream_type: MediaStreamType::AudioOnly,
    };

    assert_eq!(
        MediaStreamType::AudioOnly,
        classifier
            .classify_content(b"audio")
            .expect("classification should succeed")
    );
}

#[test]
fn test_backend_classifier_gets_default_content_and_file_entries() {
    let classifier = BackendClassifier;

    assert_eq!(
        MediaStreamType::AudioOnly,
        classifier
            .classify_content(b"audio")
            .expect("content classification should use backend content method")
    );
    assert_eq!(
        MediaStreamType::VideoOnly,
        classifier
            .classify_file(Path::new("Cargo.toml"))
            .expect("file classification should use backend local-file method")
    );
    assert!(matches!(
        classifier.classify_file(Path::new(".")),
        Err(MimeError::InvalidClassifierInput { .. })
    ));
    assert!(
        classifier
            .classify_file(Path::new("__missing_media__"))
            .is_err()
    );
}

#[test]
fn test_file_based_classifier_stages_content_to_local_file() {
    let classifier = LocalFileOnlyClassifier;

    assert_eq!(
        MediaStreamType::VideoWithAudio,
        classifier
            .classify_content(b"media")
            .expect("content should be staged to a temporary file")
    );
}

#[test]
fn test_file_based_classifier_propagates_local_file_error() {
    let classifier = FailingLocalFileOnlyClassifier;
    let mut error_reader = ErrorReader;

    assert!(matches!(
        classifier.classify_content(b"media"),
        Err(MimeError::InvalidClassifierInput { .. })
    ));
    assert!(classifier.classify_reader(&mut error_reader).is_err());
}

#[test]
fn test_boxed_media_stream_classifier_trait_object_delegates_all_entry_points() {
    let classifier: Box<dyn MediaStreamClassifier> = Box::new(StaticClassifier {
        stream_type: MediaStreamType::VideoWithAudio,
    });

    assert_media_stream_classifier_delegates(&classifier, MediaStreamType::VideoWithAudio);
}

#[test]
fn test_shared_media_stream_classifier_trait_object_delegates_all_entry_points() {
    let classifier: Arc<dyn MediaStreamClassifier> = Arc::new(StaticClassifier {
        stream_type: MediaStreamType::VideoOnly,
    });
    let cloned = classifier.clone();

    assert_media_stream_classifier_delegates(&classifier, MediaStreamType::VideoOnly);
    assert_media_stream_classifier_delegates(&cloned, MediaStreamType::VideoOnly);
}

/// Asserts that a concrete classifier handle implements and delegates the trait.
fn assert_media_stream_classifier_delegates<C>(classifier: &C, expected: MediaStreamType)
where
    C: MediaStreamClassifier,
{
    let mut reader = Cursor::new(b"media".to_vec());

    assert_eq!(
        expected,
        classifier
            .classify_file(Path::new("Cargo.toml"))
            .expect("trait-bound file classification should delegate")
    );
    assert_eq!(
        expected,
        classifier
            .classify_reader(&mut reader)
            .expect("trait-bound reader classification should delegate")
    );
    assert_eq!(
        expected,
        classifier
            .classify_content(b"media")
            .expect("trait-bound content classification should delegate")
    );
}
