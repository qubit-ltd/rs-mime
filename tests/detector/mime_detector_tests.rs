/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/

use std::sync::Arc;

use qubit_mime::{
    ArcMimeDetector,
    BoxMimeDetector,
    FileCommandMimeDetector,
    MimeConfig,
    MimeDetectionPolicy,
    MimeDetector,
    MimeResult,
    RepositoryMimeDetector,
};
use tempfile::{
    NamedTempFile,
    TempDir,
};

const CHILD_WITHOUT_FILE_COMMAND: &str = "QUBIT_MIME_CHILD_WITHOUT_FILE_COMMAND";

#[derive(Debug)]
struct StaticDetector;

impl MimeDetector for StaticDetector {
    fn detect_by_filename(&self, _filename: &str) -> Option<String> {
        Some("application/x-static-name".to_owned())
    }

    fn detect_by_content(&self, _content: &[u8]) -> Option<String> {
        Some("application/x-static-content".to_owned())
    }

    fn detect(
        &self,
        _content: &[u8],
        _filename: Option<&str>,
        _policy: MimeDetectionPolicy,
    ) -> Option<String> {
        Some("application/x-static-detect".to_owned())
    }

    fn detect_reader(
        &self,
        _reader: &mut dyn qubit_io::ReadSeek,
        _filename: Option<&str>,
        _policy: MimeDetectionPolicy,
    ) -> MimeResult<Option<String>> {
        Ok(Some("application/x-static-reader".to_owned()))
    }

    fn detect_file(
        &self,
        _file: &std::path::Path,
        _policy: MimeDetectionPolicy,
    ) -> MimeResult<Option<String>> {
        Ok(Some("application/x-static-file".to_owned()))
    }
}

#[test]
fn test_mime_detector_trait_supports_repository_detector() {
    let detector = RepositoryMimeDetector::new().expect("default repository should load");
    let detector: &dyn MimeDetector = &detector;
    assert_eq!(
        Some("image/jpeg".to_owned()),
        detector.detect_by_filename("photo.JPG")
    );
    assert_eq!(
        Some("application/pdf".to_owned()),
        detector.detect_by_content(b"%PDF-1.7\n")
    );
    assert_eq!(
        Some("application/pdf".to_owned()),
        detector.detect(
            b"%PDF-1.7\n",
            Some("photo.jpg"),
            MimeDetectionPolicy::VerifyContent,
        )
    );
}

#[test]
fn test_mime_detector_trait_supports_reader_and_file_detection() {
    let detector = RepositoryMimeDetector::new().expect("default repository should load");
    let detector: &dyn MimeDetector = &detector;
    let mut reader = std::io::Cursor::new(b"%PDF-1.7\n".to_vec());

    let from_reader = detector
        .detect_reader(
            &mut reader,
            Some("document.pdf"),
            MimeDetectionPolicy::VerifyContent,
        )
        .expect("trait-object reader detection should succeed");

    let mut file = NamedTempFile::with_suffix(".pdf").expect("temp file should be created");
    std::io::Write::write_all(&mut file, b"%PDF-1.7\n").expect("temp file should be writable");
    let from_file = detector
        .detect_file(file.path(), MimeDetectionPolicy::VerifyContent)
        .expect("trait-object file detection should succeed");

    assert_eq!(Some("application/pdf".to_owned()), from_reader);
    assert_eq!(Some("application/pdf".to_owned()), from_file);
    assert_eq!(0, reader.position());
}

#[test]
fn test_default_mime_detector_returns_usable_detector() {
    let detector = BoxMimeDetector::default();
    assert!(detector.detect_by_filename("document.pdf").is_some());
}

#[test]
fn test_mime_detector_wrappers_select_named_detectors() {
    let boxed = BoxMimeDetector::from_name("repository").expect("repository detector");
    let shared = ArcMimeDetector::from_name("repository").expect("repository detector");
    let boxed_file = BoxMimeDetector::from_name("file").expect("file detector");
    let shared_file = ArcMimeDetector::from_name("file").expect("file detector");

    assert_eq!(
        Some("application/pdf".to_owned()),
        boxed.detect_by_filename("document.pdf")
    );
    assert_eq!(
        Some("image/png".to_owned()),
        shared.detect_by_filename("image.png")
    );
    assert_eq!(
        Some("application/pdf".to_owned()),
        boxed_file.detect_by_filename("document.pdf")
    );
    assert_eq!(
        Some("image/png".to_owned()),
        shared_file.detect_by_filename("image.png")
    );
    assert!(BoxMimeDetector::from_name("unknown").is_none());
    assert!(ArcMimeDetector::from_name("unknown").is_none());
}

#[test]
fn test_box_mime_detector_wrapper_delegates_all_entry_points() {
    let wrapper = BoxMimeDetector::new(Box::new(StaticDetector));
    let mut reader = std::io::Cursor::new(b"data".to_vec());

    assert_eq!(
        Some("application/x-static-name".to_owned()),
        wrapper.as_ref().detect_by_filename("file.bin")
    );
    assert_eq!(
        Some("application/x-static-content".to_owned()),
        std::ops::Deref::deref(&wrapper).detect_by_content(b"data")
    );
    assert_eq!(
        Some("application/x-static-content".to_owned()),
        wrapper.detect_by_content(b"data")
    );
    assert_eq!(
        Some("application/x-static-detect".to_owned()),
        wrapper.detect(
            b"data",
            Some("file.bin"),
            MimeDetectionPolicy::PreferFilename
        )
    );
    assert_eq!(
        Some("application/x-static-reader".to_owned()),
        wrapper
            .detect_reader(&mut reader, None, MimeDetectionPolicy::PreferFilename)
            .expect("boxed reader delegation should succeed")
    );
    assert_eq!(
        Some("application/x-static-file".to_owned()),
        wrapper
            .detect_file(
                std::path::Path::new("Cargo.toml"),
                MimeDetectionPolicy::PreferFilename
            )
            .expect("boxed file delegation should succeed")
    );

    let trait_object: Box<dyn MimeDetector> = wrapper.into();
    assert_eq!(
        Some("application/x-static-name".to_owned()),
        trait_object.detect_by_filename("file.bin")
    );

    let from_trait = BoxMimeDetector::from(Box::new(StaticDetector) as Box<dyn MimeDetector>);
    assert_eq!(
        Some("application/x-static-content".to_owned()),
        from_trait.into_inner().detect_by_content(b"data")
    );
}

#[test]
fn test_arc_mime_detector_wrapper_delegates_all_entry_points() {
    let wrapper = ArcMimeDetector::new(Arc::new(StaticDetector));
    let mut reader = std::io::Cursor::new(b"data".to_vec());

    assert_eq!(
        Some("application/x-static-name".to_owned()),
        wrapper.as_ref().detect_by_filename("file.bin")
    );
    assert_eq!(
        Some("application/x-static-content".to_owned()),
        std::ops::Deref::deref(&wrapper).detect_by_content(b"data")
    );
    assert_eq!(
        Some("application/x-static-content".to_owned()),
        wrapper.detect_by_content(b"data")
    );
    assert_eq!(
        Some("application/x-static-detect".to_owned()),
        wrapper.detect(
            b"data",
            Some("file.bin"),
            MimeDetectionPolicy::PreferFilename
        )
    );
    assert_eq!(
        Some("application/x-static-reader".to_owned()),
        wrapper
            .detect_reader(&mut reader, None, MimeDetectionPolicy::PreferFilename)
            .expect("arc reader delegation should succeed")
    );
    assert_eq!(
        Some("application/x-static-file".to_owned()),
        wrapper
            .detect_file(
                std::path::Path::new("Cargo.toml"),
                MimeDetectionPolicy::PreferFilename
            )
            .expect("arc file delegation should succeed")
    );

    let trait_object: Arc<dyn MimeDetector> = wrapper.into();
    assert_eq!(
        Some("application/x-static-name".to_owned()),
        trait_object.detect_by_filename("file.bin")
    );

    let from_trait = ArcMimeDetector::from(Arc::new(StaticDetector) as Arc<dyn MimeDetector>);
    assert_eq!(
        Some("application/x-static-content".to_owned()),
        from_trait.into_inner().detect_by_content(b"data")
    );
}

#[test]
fn test_mime_detector_wrappers_build_from_config_defaults() {
    let config = MimeConfig::default();
    let file_config = create_detector_config("file");
    let unknown_config = create_detector_config("unknown");
    let boxed = BoxMimeDetector::from_config(&config);
    let shared = ArcMimeDetector::from_config(&config);
    let default_shared = ArcMimeDetector::default();
    let boxed_file = BoxMimeDetector::from_config(&file_config);
    let shared_file = ArcMimeDetector::from_config(&file_config);
    let fallback = BoxMimeDetector::from_config(&unknown_config);
    let repository_default = RepositoryMimeDetector::default();
    let file_default = FileCommandMimeDetector::default();
    let file_from_config = FileCommandMimeDetector::from_mime_config(file_config);

    assert!(boxed.detect_by_filename("document.pdf").is_some());
    assert!(shared.detect_by_filename("document.pdf").is_some());
    assert!(default_shared.detect_by_filename("document.pdf").is_some());
    assert!(boxed_file.detect_by_filename("document.pdf").is_some());
    assert!(shared_file.detect_by_filename("document.pdf").is_some());
    assert!(fallback.detect_by_filename("document.pdf").is_some());
    assert!(
        repository_default
            .detect_by_filename("document.pdf")
            .is_some()
    );
    assert!(file_default.detect_by_filename("document.pdf").is_some());
    assert!(
        file_from_config
            .detect_by_filename("document.pdf")
            .is_some()
    );
}

#[test]
fn test_unknown_detector_falls_back_to_repository_when_file_command_is_unavailable() {
    if std::env::var_os(CHILD_WITHOUT_FILE_COMMAND).is_some() {
        let config = create_detector_config("unknown");
        let detector = BoxMimeDetector::from_config(&config);

        assert_eq!(
            Some("application/pdf".to_owned()),
            detector.detect_by_filename("document.pdf")
        );
        return;
    }

    let temp_dir = TempDir::new().expect("empty PATH directory should be created");
    let output = std::process::Command::new(
        std::env::current_exe().expect("current test executable should be known"),
    )
    .arg("--exact")
    .arg(
        "detector::mime_detector_tests::test_unknown_detector_falls_back_to_repository_when_file_command_is_unavailable",
    )
    .arg("--nocapture")
    .env(CHILD_WITHOUT_FILE_COMMAND, "1")
    .env("PATH", temp_dir.path())
    .output()
    .expect("child test process should run");

    assert!(
        output.status.success(),
        "child test failed: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn create_detector_config(detector: &str) -> MimeConfig {
    let mut config = qubit_config::Config::new();
    config
        .set(qubit_mime::CONFIG_MIME_DETECTOR_DEFAULT, detector)
        .expect("detector default should be configurable");
    MimeConfig::from_config(&config).expect("detector config should parse")
}
