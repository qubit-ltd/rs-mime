/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/

use std::io::{Error, Read, Result as IoResult};
use std::path::Path;
use std::sync::Arc;

use qubit_mime::{
    ArcMediaStreamClassifier, BoxMediaStreamClassifier, FileBasedMediaStreamClassifier,
    MediaStreamClassifier, MediaStreamClassifierBackend, MediaStreamType, MimeConfig, MimeError,
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
fn test_default_box_media_stream_classifier_returns_classifier() {
    let classifier = BoxMediaStreamClassifier::default();
    let _ = classifier.classify_content(b"not a media file");
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
fn test_media_stream_classifier_wrappers_select_named_classifiers() {
    let boxed = BoxMediaStreamClassifier::from_name("ffprobe").expect("ffprobe classifier");
    let shared = ArcMediaStreamClassifier::from_name("ffprobe").expect("ffprobe classifier");
    let fallback_config = create_classifier_config("unknown");
    let fallback = BoxMediaStreamClassifier::from_config(&fallback_config);

    let _ = boxed.classify_content(b"not a media file");
    let _ = shared.classify_content(b"not a media file");
    let _ = fallback.classify_content(b"not a media file");
    assert!(BoxMediaStreamClassifier::from_name("unknown").is_none());
    assert!(ArcMediaStreamClassifier::from_name("unknown").is_none());
}

#[test]
fn test_box_media_stream_classifier_wrapper_delegates_all_entry_points() {
    let wrapper = BoxMediaStreamClassifier::new(Box::new(StaticClassifier {
        stream_type: MediaStreamType::VideoWithAudio,
    }));

    assert_eq!(
        MediaStreamType::VideoWithAudio,
        wrapper
            .classify_file(Path::new("Cargo.toml"))
            .expect("boxed wrapper should delegate file classification")
    );
    assert_eq!(
        MediaStreamType::VideoWithAudio,
        wrapper
            .as_ref()
            .classify_file(Path::new("Cargo.toml"))
            .expect("boxed as_ref should delegate file classification")
    );
    assert_eq!(
        MediaStreamType::VideoWithAudio,
        std::ops::Deref::deref(&wrapper)
            .classify_content(b"media")
            .expect("boxed deref should delegate content classification")
    );

    let trait_object: Box<dyn MediaStreamClassifier> = wrapper.into();
    assert_eq!(
        MediaStreamType::VideoWithAudio,
        trait_object
            .classify_content(b"media")
            .expect("boxed conversion should expose inner classifier")
    );

    let from_trait = BoxMediaStreamClassifier::from(Box::new(StaticClassifier {
        stream_type: MediaStreamType::AudioOnly,
    }) as Box<dyn MediaStreamClassifier>);
    assert_eq!(
        MediaStreamType::AudioOnly,
        from_trait
            .into_inner()
            .classify_file(Path::new("Cargo.toml"))
            .expect("into_inner should return boxed classifier")
    );
}

#[test]
fn test_arc_media_stream_classifier_wrapper_delegates_all_entry_points() {
    let wrapper = ArcMediaStreamClassifier::new(Arc::new(StaticClassifier {
        stream_type: MediaStreamType::VideoOnly,
    }));

    assert_eq!(
        MediaStreamType::VideoOnly,
        wrapper
            .as_ref()
            .classify_file(Path::new("Cargo.toml"))
            .expect("arc as_ref should delegate file classification")
    );
    assert_eq!(
        MediaStreamType::VideoOnly,
        std::ops::Deref::deref(&wrapper)
            .classify_content(b"media")
            .expect("arc deref should delegate content classification")
    );

    let trait_object: Arc<dyn MediaStreamClassifier> = wrapper.into();
    assert_eq!(
        MediaStreamType::VideoOnly,
        trait_object
            .classify_content(b"media")
            .expect("arc conversion should expose inner classifier")
    );

    let from_trait = ArcMediaStreamClassifier::from(Arc::new(StaticClassifier {
        stream_type: MediaStreamType::AudioOnly,
    }) as Arc<dyn MediaStreamClassifier>);
    assert_eq!(
        MediaStreamType::AudioOnly,
        from_trait
            .into_inner()
            .classify_file(Path::new("Cargo.toml"))
            .expect("into_inner should return shared classifier")
    );
}

#[test]
fn test_media_stream_classifier_wrappers_build_from_config_defaults() {
    let config = MimeConfig::default();

    let boxed = BoxMediaStreamClassifier::from_config(&config);
    let shared = ArcMediaStreamClassifier::from_config(&config);
    let default_shared = ArcMediaStreamClassifier::default();

    let _ = boxed.classify_content(b"not a media file");
    let _ = shared.classify_content(b"not a media file");
    let _ = default_shared.classify_content(b"not a media file");
}

fn create_classifier_config(classifier: &str) -> MimeConfig {
    let mut config = qubit_config::Config::new();
    config
        .set(
            qubit_mime::CONFIG_MEDIA_STREAM_CLASSIFIER_DEFAULT,
            classifier,
        )
        .expect("classifier default should be configurable");
    MimeConfig::from_config(&config).expect("classifier fallback config should parse")
}
