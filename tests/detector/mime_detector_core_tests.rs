// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io::{Cursor, Read};
use std::path::Path;
use std::sync::Arc;

use qubit_mime::{
    DetectionSource, MediaStreamClassifier, MediaStreamType, MimeConfig, MimeDetectionPolicy,
    MimeDetectorCore, MimeError,
};

#[derive(Debug)]
struct StaticClassifier {
    stream_type: MediaStreamType,
    expected_first_byte: Option<u8>,
}

#[derive(Debug)]
struct FailingClassifier;

impl MediaStreamClassifier for StaticClassifier {
    fn classify_file(&self, _file: &Path) -> Result<MediaStreamType, MimeError> {
        Ok(self.stream_type)
    }

    fn classify_reader(&self, reader: &mut dyn Read) -> Result<MediaStreamType, MimeError> {
        if let Some(expected_first_byte) = self.expected_first_byte {
            let mut buffer = [0_u8; 1];
            reader.read_exact(&mut buffer)?;
            if buffer[0] != expected_first_byte {
                return Err(MimeError::InvalidClassifierInput {
                    reason: "reader did not start at the beginning".to_owned(),
                });
            }
        }
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
        expected_first_byte: None,
    })));

    let refined = detector.refine_detected_mime_type(
        "video/webm",
        Some("sample.webm"),
        DetectionSource::Content(b"webm"),
    );

    assert_eq!("audio/webm", refined.expect("refinement should succeed"));
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
        expected_first_byte: None,
    })));

    assert_eq!(
        Some("image/jpeg".to_owned()),
        detector
            .select_result(
            &["image/jpeg".to_owned()],
            &["video/webm".to_owned()],
            Some("movie.webm"),
            MimeDetectionPolicy::PreferFilename,
            DetectionSource::Content(b"webm"),
        )
            .expect("selection should succeed")
    );
    assert_eq!(
        Some("video/webm".to_owned()),
        detector
            .select_result(
            &["video/webm".to_owned()],
            &["audio/webm".to_owned()],
            Some("movie.webm"),
            MimeDetectionPolicy::VerifyContent,
            DetectionSource::Content(b"webm"),
        )
            .expect("selection should succeed")
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
        expected_first_byte: None,
    })));

    assert!(detector.media_stream_classifier().is_some());
    assert_eq!(
        "video/ogg",
        detector
            .refine_detected_mime_type(
            "audio/ogg",
            None,
            DetectionSource::Path(Path::new("Cargo.toml")),
        )
            .expect("path refinement should succeed")
    );
    assert_eq!(
        "video/webm",
        detector
            .refine_detected_mime_type("video/webm", Some("movie.webm"), DetectionSource::None,)
            .expect("refinement without source should succeed")
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
        .expect("refinement without classifier should succeed")
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
        .expect("disabled refinement should succeed")
    );
    assert_eq!(
        "video/webm",
        MimeDetectorCore::new(create_precise_config(true, "webm", ""))
            .refine_detected_mime_type(
                "video/webm",
                Some("movie.webm"),
                DetectionSource::Content(b""),
            )
            .expect("unmapped refinement should succeed")
    );
    assert_eq!(
        "application/pdf",
        detector
            .refine_detected_mime_type(
            "application/pdf",
            Some("movie.webm"),
            DetectionSource::Content(b""),
        )
            .expect("unrelated MIME refinement should succeed")
    );

    let mut failing = MimeDetectorCore::new(create_precise_config(
        true,
        "webm",
        "webm:video/webm,audio/webm",
    ));
    failing.set_media_stream_classifier(Some(Arc::new(FailingClassifier)));

    assert!(matches!(
        failing
            .refine_detected_mime_type(
            "video/webm",
            Some("movie.webm"),
            DetectionSource::Content(b""),
        )
            .expect_err("content refinement must retain classifier failures"),
        MimeError::InvalidClassifierInput { .. }
    ));
    assert!(matches!(
        failing
            .refine_detected_mime_type(
            "video/webm",
            Some("movie.webm"),
            DetectionSource::Path(Path::new("Cargo.toml")),
        )
            .expect_err("path refinement must retain classifier failures"),
        MimeError::InvalidClassifierInput { .. }
    ));
}

/// Verifies reader-based refinement classifies the stream and restores its
/// original position.
#[test]
fn test_select_reader_result_refines_without_consuming_reader_position() {
    let mut detector = MimeDetectorCore::new(create_precise_config(
        true,
        "webm",
        "webm:video/webm,audio/webm",
    ));
    detector.set_media_stream_classifier(Some(Arc::new(StaticClassifier {
        stream_type: MediaStreamType::AudioOnly,
        expected_first_byte: Some(b'w'),
    })));
    let mut reader = Cursor::new(b"webm".to_vec());
    reader.set_position(1);

    let refined = detector
        .select_reader_result(
            &[],
            &["video/webm".to_owned()],
            Some("sample.webm"),
            MimeDetectionPolicy::VerifyContent,
            &mut reader,
        )
        .expect("reader refinement should succeed");

    assert_eq!(Some("audio/webm".to_owned()), refined);
    assert_eq!(1, reader.position());
}

/// Verifies reader-based refinement propagates classifier failures and restores
/// the original reader position.
#[test]
fn test_select_reader_result_propagates_classifier_failures() {
    let mut detector = MimeDetectorCore::new(create_precise_config(
        true,
        "webm",
        "webm:video/webm,audio/webm",
    ));
    detector.set_media_stream_classifier(Some(Arc::new(FailingClassifier)));
    let mut reader = Cursor::new(b"webm".to_vec());
    reader.set_position(1);

    let error = detector
        .select_reader_result(
            &[],
            &["video/webm".to_owned()],
            Some("sample.webm"),
            MimeDetectionPolicy::VerifyContent,
            &mut reader,
        )
        .expect_err("reader refinement must retain classifier failures");

    assert!(matches!(error, MimeError::InvalidClassifierInput { .. }));
    assert_eq!(1, reader.position());
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
