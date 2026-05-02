/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/

use std::path::Path;

use qubit_mime::{
    ArcMediaStreamClassifier, BoxMediaStreamClassifier, MediaStreamClassifier, MediaStreamType,
    MimeError,
};

#[derive(Debug)]
struct StaticClassifier {
    stream_type: MediaStreamType,
}

impl MediaStreamClassifier for StaticClassifier {
    fn classify_file(&self, _file: &Path) -> Result<MediaStreamType, MimeError> {
        Ok(self.stream_type)
    }

    fn classify_content(&self, _content: &[u8]) -> Result<MediaStreamType, MimeError> {
        Ok(self.stream_type)
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
