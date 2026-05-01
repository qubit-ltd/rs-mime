/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/

use std::path::Path;
use std::sync::Arc;

use qubit_mime::{
    AbstractMimeDetector, DetectionSource, MediaStreamClassifier, MediaStreamType, MimeError,
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
fn test_merge_results_matches_java_detector_strategy() {
    let detector = AbstractMimeDetector::default();

    assert_eq!(
        None,
        detector.merge_results(&[], &[]),
        "no filename or content candidate should return none"
    );
    assert_eq!(
        Some("image/png".to_owned()),
        detector.merge_results(&["image/png".to_owned()], &[])
    );
    assert_eq!(
        Some("application/pdf".to_owned()),
        detector.merge_results(&[], &["application/pdf".to_owned()])
    );
    assert_eq!(
        Some("application/pdf".to_owned()),
        detector.merge_results(
            &["application/pdf".to_owned(), "text/plain".to_owned()],
            &["application/pdf".to_owned()]
        )
    );
    assert_eq!(
        Some("application/pdf".to_owned()),
        detector.merge_results(&["image/jpeg".to_owned()], &["application/pdf".to_owned()])
    );
}

#[test]
fn test_refine_detected_mime_type_uses_media_stream_classifier() {
    let mut detector = AbstractMimeDetector::default();
    detector.set_media_stream_classifier(Some(Arc::new(StaticClassifier {
        stream_type: MediaStreamType::AudioOnly,
    })));

    let refined = detector.refine_detected_mime_type(
        "video/webm",
        Some("sample.webm"),
        DetectionSource::Content(b"webm"),
    );

    assert_eq!("audio/webm", refined);
}
