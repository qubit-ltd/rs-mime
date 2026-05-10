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
use std::sync::Arc;

use qubit_mime::{
    DetectionSource,
    MediaStreamClassifier,
    MediaStreamType,
    MimeConfig,
    MimeDetectionPolicy,
    MimeDetectorCore,
    MimeError,
};

#[derive(Debug)]
struct StaticClassifier {
    stream_type: MediaStreamType,
}

#[derive(Debug)]
struct FailingClassifier;

impl MediaStreamClassifier for StaticClassifier {
    fn classify_file(&self, _file: &Path) -> Result<MediaStreamType, MimeError> {
        Ok(self.stream_type)
    }

    fn classify_reader(&self, _reader: &mut dyn Read) -> Result<MediaStreamType, MimeError> {
        Ok(self.stream_type)
    }
}

impl MediaStreamClassifier for FailingClassifier {
    fn classify_file(&self, _file: &Path) -> Result<MediaStreamType, MimeError> {
        Err(MimeError::InvalidClassifierInput {
            reason: "forced".to_owned(),
        })
    }

    fn classify_reader(&self, _reader: &mut dyn Read) -> Result<MediaStreamType, MimeError> {
        Err(MimeError::InvalidClassifierInput {
            reason: "forced".to_owned(),
        })
    }
}

#[test]
fn test_merge_results_uses_detector_selection_strategy() {
    let detector = MimeDetectorCore::default();

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
    let mut detector = MimeDetectorCore::new(create_precise_config(
        true,
        "webm",
        "webm:video/webm,audio/webm",
    ));
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

#[test]
fn test_select_result_honors_prefer_filename_and_refines_content_result() {
    let mut detector = MimeDetectorCore::new(create_precise_config(
        true,
        "webm",
        "webm:video/webm,audio/webm",
    ));
    detector.set_media_stream_classifier(Some(Arc::new(StaticClassifier {
        stream_type: MediaStreamType::VideoOnly,
    })));

    assert_eq!(
        Some("image/jpeg".to_owned()),
        detector.select_result(
            &["image/jpeg".to_owned()],
            &["video/webm".to_owned()],
            Some("movie.webm"),
            MimeDetectionPolicy::PreferFilename,
            DetectionSource::Content(b"webm"),
        )
    );
    assert_eq!(
        Some("video/webm".to_owned()),
        detector.select_result(
            &["video/webm".to_owned()],
            &["audio/webm".to_owned()],
            Some("movie.webm"),
            MimeDetectionPolicy::VerifyContent,
            DetectionSource::Content(b"webm"),
        )
    );
}

#[test]
fn test_refine_detected_mime_type_handles_disabled_missing_and_failing_cases() {
    let mut detector = MimeDetectorCore::new(create_precise_config(
        true,
        "webm,ogg",
        "webm:video/webm,audio/webm;ogg:video/ogg,audio/ogg",
    ));
    detector.set_media_stream_classifier(Some(Arc::new(StaticClassifier {
        stream_type: MediaStreamType::VideoOnly,
    })));

    assert!(detector.media_stream_classifier().is_some());
    assert_eq!(
        "video/ogg",
        detector.refine_detected_mime_type(
            "audio/ogg",
            None,
            DetectionSource::Path(Path::new("Cargo.toml")),
        )
    );
    assert_eq!(
        "video/webm",
        detector
            .refine_detected_mime_type("video/webm", Some("movie.webm"), DetectionSource::None,)
    );
    assert_eq!(
        "video/webm",
        MimeDetectorCore::new(create_precise_config(
            true,
            "webm",
            "webm:video/webm,audio/webm"
        ))
        .refine_detected_mime_type(
            "video/webm",
            Some("movie.webm"),
            DetectionSource::Content(b""),
        )
    );
    assert_eq!(
        "video/webm",
        MimeDetectorCore::new(create_precise_config(
            false,
            "webm",
            "webm:video/webm,audio/webm"
        ))
        .refine_detected_mime_type(
            "video/webm",
            Some("movie.webm"),
            DetectionSource::Content(b""),
        )
    );
    assert_eq!(
        "video/webm",
        MimeDetectorCore::new(create_precise_config(true, "webm", "")).refine_detected_mime_type(
            "video/webm",
            Some("movie.webm"),
            DetectionSource::Content(b""),
        )
    );
    assert_eq!(
        "application/pdf",
        detector.refine_detected_mime_type(
            "application/pdf",
            Some("movie.webm"),
            DetectionSource::Content(b""),
        )
    );

    let mut failing = MimeDetectorCore::new(create_precise_config(
        true,
        "webm",
        "webm:video/webm,audio/webm",
    ));
    failing.set_media_stream_classifier(Some(Arc::new(FailingClassifier)));

    assert_eq!(
        "video/webm",
        failing.refine_detected_mime_type(
            "video/webm",
            Some("movie.webm"),
            DetectionSource::Content(b""),
        )
    );
    assert_eq!(
        "video/webm",
        failing.refine_detected_mime_type(
            "video/webm",
            Some("movie.webm"),
            DetectionSource::Path(Path::new("Cargo.toml")),
        )
    );
}

fn create_precise_config(
    enable_precise_detection: bool,
    precise_detection_patterns: &str,
    ambiguous_mime_mapping: &str,
) -> MimeConfig {
    let mut config = qubit_config::Config::new();
    config
        .set(qubit_mime::CONFIG_MIME_DETECTOR_DEFAULT, "repository")
        .expect("detector selector should be configurable");
    config
        .set(
            qubit_mime::CONFIG_MEDIA_STREAM_CLASSIFIER_DEFAULT,
            "ffprobe",
        )
        .expect("classifier selector should be configurable");
    config
        .set(
            qubit_mime::CONFIG_MIME_ENABLE_PRECISE_DETECTION,
            enable_precise_detection,
        )
        .expect("precise detection flag should be configurable");
    config
        .set(
            qubit_mime::CONFIG_MIME_PRECISE_DETECTION_PATTERNS,
            precise_detection_patterns,
        )
        .expect("precise detection patterns should be configurable");
    config
        .set(
            qubit_mime::CONFIG_MIME_AMBIGUOUS_MIME_MAPPING,
            ambiguous_mime_mapping,
        )
        .expect("ambiguous MIME mapping should be configurable");
    MimeConfig::from_config(&config).expect("precise MIME config should parse")
}
