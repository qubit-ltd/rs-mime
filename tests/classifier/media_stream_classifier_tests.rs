/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/

use std::io::Read;
use std::path::Path;

use qubit_mime::{
    ArcMediaStreamClassifier, BoxMediaStreamClassifier, FileBasedMediaStreamClassifier,
    MediaStreamClassifier, MediaStreamClassifierBackend, MediaStreamType, MimeError, MimeResult,
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
fn test_default_box_media_stream_classifier_returns_classifier() {
    let classifier = BoxMediaStreamClassifier::default();
    assert!(matches!(
        classifier.classify_content(b"not a media file"),
        Ok(MediaStreamType::None) | Err(_)
    ));
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
fn test_media_stream_classifier_wrappers_select_named_classifiers() {
    let boxed = BoxMediaStreamClassifier::from_name("ffprobe").expect("ffprobe classifier");
    let shared = ArcMediaStreamClassifier::from_name("ffprobe").expect("ffprobe classifier");

    assert!(matches!(
        boxed.classify_content(b"not a media file"),
        Ok(MediaStreamType::None) | Err(_)
    ));
    assert!(matches!(
        shared.classify_content(b"not a media file"),
        Ok(MediaStreamType::None) | Err(_)
    ));
    assert!(BoxMediaStreamClassifier::from_name("unknown").is_none());
    assert!(ArcMediaStreamClassifier::from_name("unknown").is_none());
}
