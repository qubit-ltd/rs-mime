/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/

use std::path::Path;

use qubit_mime::{MediaStreamClassifier, MediaStreamType, MimeError};

#[derive(Debug)]
struct StaticClassifier {
    stream_type: MediaStreamType,
}

impl MediaStreamClassifier for StaticClassifier {
    fn classify_path(&self, _path: &Path) -> Result<MediaStreamType, MimeError> {
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
fn test_default_media_stream_classifier_is_optional() {
    let classifier = <dyn MediaStreamClassifier>::default_classifier();
    if let Some(classifier) = classifier {
        assert!(matches!(
            classifier.classify_content(b"not a media file"),
            Ok(MediaStreamType::None) | Err(_)
        ));
    }
}
