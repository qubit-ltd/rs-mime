// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io::Cursor;
use std::io::Read;
use std::path::Path;
use std::sync::Arc;

use qubit_config::Config;
use qubit_mime::CONFIG_MEDIA_STREAM_CLASSIFIER_DEFAULT;
use qubit_mime::CONFIG_MIME_AMBIGUOUS_MIME_MAPPING;
use qubit_mime::CONFIG_MIME_DETECTOR_DEFAULT;
use qubit_mime::CONFIG_MIME_ENABLE_PRECISE_DETECTION;
use qubit_mime::CONFIG_MIME_PRECISE_DETECTION_PATTERNS;
use qubit_mime::DetectionSource;
use qubit_mime::MediaStreamClassifier;
use qubit_mime::MediaStreamType;
use qubit_mime::MimeConfig;
use qubit_mime::MimeDetectionPolicy;
use qubit_mime::MimeDetectorCore;
use qubit_mime::MimeError;
use qubit_mime::MimeRepository;
use qubit_mime::RepositoryMimeDetector;

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
fn test_repository_detector_core_uses_its_repository_relations() {
    let repository = MimeRepository::from_xml(
        r#"<mime-info xmlns="http://www.freedesktop.org/standards/shared-mime-info">
<mime-type type="application/x-child"/>
<mime-type type="application/x-alias-child">
  <alias type="application/x-child-alias"/>
  <sub-class-of type="application/x-parent-alias"/>
</mime-type>
<mime-type type="application/x-parent"><alias type="application/x-parent-alias"/></mime-type>
</mime-info>"#,
    )
    .expect("custom repository should parse");
    let detector = RepositoryMimeDetector::with_repository(&repository);

    assert_eq!(
        Some("application/x-child-alias".to_owned()),
        detector.core().merge_results(
            &["application/x-child-alias".to_owned()],
            &["application/x-parent".to_owned()]
        )
    );
    assert_eq!(
        Some("application/x-alias-child".to_owned()),
        detector.core().merge_results(
            &["application/x-parent".to_owned()],
            &["application/x-alias-child".to_owned()]
        ),
        "inheritance must not be checked from content child to filename parent"
    );
    assert_eq!(
        Some("application/x-parent".to_owned()),
        detector.core().merge_results(
            &["application/x-unrelated".to_owned()],
            &["application/x-parent".to_owned()]
        ),
        "unknown names do not gain inferred inheritance"
    );

    let empty_repository = MimeRepository::empty();
    let empty_detector = RepositoryMimeDetector::with_repository(&empty_repository);
    assert_eq!(
        Some("application/zip".to_owned()),
        empty_detector
            .core()
            .merge_results(&["application/epub+zip".to_owned()], &["application/zip".to_owned()]),
        "a custom empty repository must not borrow bundled relationships"
    );

    let filename = ["application/x-child-alias".to_owned()];
    let content = ["application/x-parent".to_owned()];
    assert_eq!(
        Some("application/x-child-alias".to_owned()),
        detector
            .core()
            .select_result(
                &filename,
                &content,
                None,
                MimeDetectionPolicy::Freedesktop,
                DetectionSource::None,
            )
            .expect("Freedesktop selection should succeed")
    );
    for policy in [MimeDetectionPolicy::ContentFirst, MimeDetectionPolicy::VerifyContent] {
        assert_eq!(
            Some("application/x-parent".to_owned()),
            detector
                .core()
                .select_result(&filename, &content, None, policy, DetectionSource::None)
                .expect("non-Freedesktop selection should succeed")
        );
    }
    assert_eq!(
        Some("application/x-child-alias".to_owned()),
        detector
            .core()
            .select_result(
                &filename,
                &content,
                None,
                MimeDetectionPolicy::PreferFilename,
                DetectionSource::None,
            )
            .expect("PreferFilename selection should succeed")
    );
    assert_eq!(
        None,
        detector
            .core()
            .select_result(
                &filename,
                &[],
                None,
                MimeDetectionPolicy::VerifyContent,
                DetectionSource::None,
            )
            .expect("VerifyContent selection should succeed")
    );
}

#[test]
fn test_unknown_text_does_not_inherit_plain() {
    let repository = MimeRepository::empty();
    let detector = RepositoryMimeDetector::with_repository(&repository);
    assert_eq!(
        Some("text/plain".to_owned()),
        detector
            .core()
            .merge_results(&["text/x-unregistered".to_owned()], &["text/plain".to_owned()],),
    );
}

#[test]
fn test_text_inheritance_requires_known_name_but_keeps_explicit_edges() {
    let repository = MimeRepository::from_xml(
        r#"<mime-info xmlns="http://www.freedesktop.org/standards/shared-mime-info">
<mime-type type="text/x-known"><alias type="text/x-known-alias"/></mime-type>
<mime-type type="application/x-inherits-unknown-text"><sub-class-of type="text/x-parent"/></mime-type>
<mime-type type="application/x-explicit-child"><sub-class-of type="application/x-unknown-parent"/></mime-type>
</mime-info>"#,
    )
    .expect("custom repository should parse");
    let detector = RepositoryMimeDetector::with_repository(&repository);
    let merge = |filename: &str, content: &str| {
        detector
            .core()
            .merge_results(&[filename.to_owned()], &[content.to_owned()])
    };

    assert_eq!(
        Some("text/plain".to_owned()),
        merge("text/x-unregistered", "text/plain"),
        "unknown names in a nonempty repository must not gain inferred inheritance"
    );
    assert_eq!(Some("text/x-known".to_owned()), merge("text/x-known", "text/plain"));
    assert_eq!(
        Some("text/x-known-alias".to_owned()),
        merge("text/x-known-alias", "text/plain")
    );
    assert_eq!(
        Some("TEXT/X-UNREGISTERED".to_owned()),
        merge("TEXT/X-UNREGISTERED", "text/x-unregistered")
    );
    assert_eq!(
        Some("application/x-inherits-unknown-text".to_owned()),
        merge("text/plain", "application/x-inherits-unknown-text"),
        "an unknown text parent must not imply text/plain"
    );
    assert_eq!(
        Some("application/x-explicit-child".to_owned()),
        merge("application/x-explicit-child", "application/x-unknown-parent"),
        "explicit edges to unknown parent names must remain effective"
    );
}

#[test]
fn test_candidate_policy_matrix() {
    let core = MimeDetectorCore::default();
    let filename = ["application/pdf".to_owned()];
    let content = ["image/png".to_owned()];
    let filename_many = ["application/pdf".to_owned(), "image/png".to_owned()];
    let content_many = ["image/png".to_owned(), "text/plain".to_owned()];
    let empty: [String; 0] = [];

    for policy in [MimeDetectionPolicy::Freedesktop, MimeDetectionPolicy::ContentFirst] {
        assert_eq!(
            Some("application/pdf".to_owned()),
            core.select_result(&filename, &empty, None, policy, DetectionSource::None)
                .unwrap()
        );
        assert_eq!(
            Some("image/png".to_owned()),
            core.select_result(&empty, &content, None, policy, DetectionSource::None)
                .unwrap()
        );
        assert_eq!(
            Some("image/png".to_owned()),
            core.select_result(&filename, &content, None, policy, DetectionSource::None)
                .unwrap()
        );
        assert_eq!(
            None,
            core.select_result(&empty, &empty, None, policy, DetectionSource::None)
                .unwrap()
        );
        assert_eq!(
            Some("image/png".to_owned()),
            core.select_result(&filename_many, &content_many, None, policy, DetectionSource::None)
                .unwrap()
        );
        assert_eq!(
            Some("image/png".to_owned()),
            core.select_result(&filename, &content_many, None, policy, DetectionSource::None)
                .unwrap()
        );
    }
    assert_eq!(
        None,
        core.select_result(
            &filename,
            &empty,
            None,
            MimeDetectionPolicy::VerifyContent,
            DetectionSource::None
        )
        .unwrap()
    );
    assert_eq!(
        Some("image/png".to_owned()),
        core.select_result(
            &filename,
            &content,
            None,
            MimeDetectionPolicy::VerifyContent,
            DetectionSource::None
        )
        .unwrap()
    );
    assert_eq!(
        Some("application/pdf".to_owned()),
        core.select_result(
            &filename,
            &content,
            None,
            MimeDetectionPolicy::PreferFilename,
            DetectionSource::None
        )
        .unwrap()
    );
    assert_eq!(
        Some("image/png".to_owned()),
        core.select_result(
            &filename_many,
            &content_many,
            None,
            MimeDetectionPolicy::PreferFilename,
            DetectionSource::None
        )
        .unwrap()
    );
}

#[test]
fn test_refine_detected_mime_type_uses_media_stream_classifier() {
    let mut detector = MimeDetectorCore::new(create_precise_config(true, "webm", "webm:video/webm,audio/webm"));
    detector.set_media_stream_classifier(Some(Arc::new(StaticClassifier {
        stream_type: MediaStreamType::AudioOnly,
        expected_first_byte: None,
    })));

    let refined =
        detector.refine_detected_mime_type("video/webm", Some("sample.webm"), DetectionSource::Content(b"webm"));

    assert_eq!("audio/webm", refined.expect("refinement should succeed"));
}

#[test]
fn test_select_result_honors_prefer_filename_and_refines_content_result() {
    let mut detector = MimeDetectorCore::new(create_precise_config(true, "webm", "webm:video/webm,audio/webm"));
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
fn test_select_result_prefer_filename_skips_classifier_for_content_and_path_sources() {
    let mut detector = MimeDetectorCore::new(create_precise_config(true, "webm", "webm:video/webm,audio/webm"));
    detector.set_media_stream_classifier(Some(Arc::new(FailingClassifier)));
    let filename = ["video/webm".to_owned()];

    for source in [
        DetectionSource::Content(b"webm"),
        DetectionSource::Path(Path::new("movie.webm")),
        DetectionSource::Prefix(b"webm"),
        DetectionSource::None,
    ] {
        assert_eq!(
            Some("video/webm".to_owned()),
            detector
                .select_result(
                    &filename,
                    &[],
                    Some("movie.webm"),
                    MimeDetectionPolicy::PreferFilename,
                    source
                )
                .expect("PreferFilename should preserve its unique filename candidate without classification")
        );
    }
}

#[test]
fn test_select_reader_result_prefer_filename_skips_classifier_and_verify_content_propagates_it() {
    let mut detector = MimeDetectorCore::new(create_precise_config(true, "webm", "webm:video/webm,audio/webm"));
    detector.set_media_stream_classifier(Some(Arc::new(FailingClassifier)));
    let filename = ["video/webm".to_owned()];
    let mut reader = Cursor::new(b"webm".to_vec());

    assert_eq!(
        Some("video/webm".to_owned()),
        detector
            .select_reader_result(
                &filename,
                &[],
                Some("movie.webm"),
                MimeDetectionPolicy::PreferFilename,
                &mut reader,
            )
            .expect("PreferFilename should skip reader classification")
    );

    let error = detector
        .select_reader_result(
            &filename,
            &["video/webm".to_owned()],
            Some("movie.webm"),
            MimeDetectionPolicy::VerifyContent,
            &mut reader,
        )
        .expect_err("VerifyContent must run and propagate classifier errors");
    assert!(matches!(error, MimeError::InvalidClassifierInput { .. }));
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
            .refine_detected_mime_type("audio/ogg", None, DetectionSource::Path(Path::new("Cargo.toml")),)
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
        MimeDetectorCore::new(create_precise_config(true, "webm", "webm:video/webm,audio/webm"))
            .refine_detected_mime_type("video/webm", Some("movie.webm"), DetectionSource::Content(b""),)
            .expect("refinement without classifier should succeed")
    );
    assert_eq!(
        "video/webm",
        MimeDetectorCore::new(create_precise_config(false, "webm", "webm:video/webm,audio/webm"))
            .refine_detected_mime_type("video/webm", Some("movie.webm"), DetectionSource::Content(b""),)
            .expect("disabled refinement should succeed")
    );
    assert_eq!(
        "video/webm",
        MimeDetectorCore::new(create_precise_config(true, "webm", ""))
            .refine_detected_mime_type("video/webm", Some("movie.webm"), DetectionSource::Content(b""),)
            .expect("unmapped refinement should succeed")
    );
    assert_eq!(
        "application/pdf",
        detector
            .refine_detected_mime_type("application/pdf", Some("movie.webm"), DetectionSource::Content(b""),)
            .expect("unrelated MIME refinement should succeed")
    );

    let mut failing = MimeDetectorCore::new(create_precise_config(true, "webm", "webm:video/webm,audio/webm"));
    failing.set_media_stream_classifier(Some(Arc::new(FailingClassifier)));

    assert!(matches!(
        failing
            .refine_detected_mime_type("video/webm", Some("movie.webm"), DetectionSource::Content(b""),)
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
    let mut detector = MimeDetectorCore::new(create_precise_config(true, "webm", "webm:video/webm,audio/webm"));
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
    let mut detector = MimeDetectorCore::new(create_precise_config(true, "webm", "webm:video/webm,audio/webm"));
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
    let mut config = Config::new();
    config
        .set(CONFIG_MIME_DETECTOR_DEFAULT, "repository")
        .expect("detector selector should be configurable");
    config
        .set(CONFIG_MEDIA_STREAM_CLASSIFIER_DEFAULT, "ffprobe")
        .expect("classifier selector should be configurable");
    config
        .set(CONFIG_MIME_ENABLE_PRECISE_DETECTION, enable_precise_detection)
        .expect("precise detection flag should be configurable");
    config
        .set(CONFIG_MIME_PRECISE_DETECTION_PATTERNS, precise_detection_patterns)
        .expect("precise detection patterns should be configurable");
    config
        .set(CONFIG_MIME_AMBIGUOUS_MIME_MAPPING, ambiguous_mime_mapping)
        .expect("ambiguous MIME mapping should be configurable");
    MimeConfig::from_config(&config).expect("precise MIME config should parse")
}

#[test]
fn test_verify_content_with_empty_candidates_returns_none() {
    let core = MimeDetectorCore::new(MimeConfig::default());
    let result = core
        .select_result(
            &["text/plain".to_owned()],
            &[],
            Some("note.txt"),
            MimeDetectionPolicy::VerifyContent,
            DetectionSource::None,
        )
        .expect("empty content candidates should be a valid result");
    assert_eq!(None, result);
}

#[test]
fn test_refine_without_filename_preserves_mime_for_conflicting_mappings() {
    for mapping in [
        "webm:video/webm,audio/webm;alt:video/webm,audio/ogg",
        "alt:video/webm,audio/ogg;webm:video/webm,audio/webm",
    ] {
        let mut detector = MimeDetectorCore::new(create_precise_config(true, "webm,alt", mapping));
        detector.set_media_stream_classifier(Some(Arc::new(FailingClassifier)));
        assert_eq!(
            "video/webm",
            detector
                .refine_detected_mime_type("video/webm", None, DetectionSource::Content(b"data"))
                .expect("ambiguous mappings must skip classification")
        );
    }
}

#[test]
fn test_refine_mapping_keeps_unique_pairs_and_filename_resolution() {
    let classifier = || {
        Arc::new(StaticClassifier {
            stream_type: MediaStreamType::AudioOnly,
            expected_first_byte: None,
        }) as Arc<dyn MediaStreamClassifier>
    };

    let mut unique = MimeDetectorCore::new(create_precise_config(true, "webm", "webm:video/webm,audio/webm"));
    unique.set_media_stream_classifier(Some(classifier()));
    assert_eq!(
        "audio/webm",
        unique
            .refine_detected_mime_type("video/webm", None, DetectionSource::Content(b"data"))
            .expect("a unique pair should still refine")
    );

    let mut same_pair = MimeDetectorCore::new(create_precise_config(
        true,
        "webm,alt",
        "webm:video/webm,audio/webm;alt:video/webm,audio/webm",
    ));
    same_pair.set_media_stream_classifier(Some(classifier()));
    assert_eq!(
        "audio/webm",
        same_pair
            .refine_detected_mime_type("video/webm", None, DetectionSource::Content(b"data"))
            .expect("duplicate keys with the same pair should refine")
    );

    let mut explicit = MimeDetectorCore::new(create_precise_config(
        true,
        "webm,alt",
        "webm:video/webm,audio/webm;alt:video/webm,audio/ogg",
    ));
    explicit.set_media_stream_classifier(Some(classifier()));
    assert_eq!(
        "audio/webm",
        explicit
            .refine_detected_mime_type("video/webm", Some("sample.webm"), DetectionSource::Content(b"data"),)
            .unwrap()
    );
    assert_eq!(
        "audio/ogg",
        explicit
            .refine_detected_mime_type("video/webm", Some("sample.alt"), DetectionSource::Content(b"data"),)
            .unwrap()
    );
}
