// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_mime::{
    CONFIG_MIME_DETECTOR_FALLBACKS,
    FileCommandMimeDetector,
    MimeConfig,
    MimeDetectionPolicy,
    MimeDetector,
    MimeDetectorBackend,
    MimeDetectorCore,
    MimeDetectorRegistry,
    MimeError,
    MimeResult,
    RepositoryMimeDetector,
};
use tempfile::NamedTempFile;

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

#[derive(Debug)]
struct DirectBackendDetector {
    core: MimeDetectorCore,
}

impl DirectBackendDetector {
    /// Creates a detector that exercises the backend default methods directly.
    fn new() -> Self {
        Self {
            core: MimeDetectorCore::default(),
        }
    }
}

impl MimeDetectorBackend for DirectBackendDetector {
    /// Gets the shared detector core.
    fn core(&self) -> &MimeDetectorCore {
        &self.core
    }

    /// Gets the content prefix length used by backend defaults.
    fn max_test_bytes(&self) -> usize {
        5
    }

    /// Recognizes plain text filenames.
    fn guess_from_filename(&self, filename: &str) -> Vec<String> {
        if filename.ends_with(".txt") {
            vec!["text/plain".to_owned()]
        } else {
            Vec::new()
        }
    }

    /// Recognizes the content prefix read by backend defaults.
    fn guess_from_content(&self, content: &[u8]) -> MimeResult<Vec<String>> {
        if content == b"hello" {
            Ok(vec!["text/plain".to_owned()])
        } else {
            Ok(Vec::new())
        }
    }
}

#[test]
fn test_mime_detector_trait_supports_repository_detector() {
    let detector =
        RepositoryMimeDetector::new().expect("default repository should load");
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
    let detector =
        RepositoryMimeDetector::new().expect("default repository should load");
    let detector: &dyn MimeDetector = &detector;
    let mut reader = std::io::Cursor::new(b"%PDF-1.7\n".to_vec());

    let from_reader = detector
        .detect_reader(
            &mut reader,
            Some("document.pdf"),
            MimeDetectionPolicy::VerifyContent,
        )
        .expect("trait-object reader detection should succeed");

    let mut file = NamedTempFile::with_suffix(".pdf")
        .expect("temp file should be created");
    std::io::Write::write_all(&mut file, b"%PDF-1.7\n")
        .expect("temp file should be writable");
    let from_file = detector
        .detect_file(file.path(), MimeDetectionPolicy::VerifyContent)
        .expect("trait-object file detection should succeed");

    assert_eq!(Some("application/pdf".to_owned()), from_reader);
    assert_eq!(Some("application/pdf".to_owned()), from_file);
    assert_eq!(0, reader.position());
}

#[test]
fn test_mime_detector_backend_defaults_read_reader_and_file_prefix() {
    let detector = DirectBackendDetector::new();
    let mut reader = std::io::Cursor::new(b"hello world".to_vec());

    let (reader_candidates, reader_content) =
        MimeDetectorBackend::guess_from_reader(&detector, &mut reader)
            .expect("backend reader default should read content prefix");

    let mut file = NamedTempFile::new().expect("temp file should be created");
    std::io::Write::write_all(&mut file, b"hello world")
        .expect("temp file should be writable");
    let (file_candidates, file_content) =
        MimeDetectorBackend::guess_from_file(&detector, file.path())
            .expect("backend file default should read content prefix");

    assert_eq!(vec!["text/plain".to_owned()], reader_candidates);
    assert_eq!(b"hello".to_vec(), reader_content);
    assert_eq!(vec!["text/plain".to_owned()], file_candidates);
    assert_eq!(b"hello".to_vec(), file_content);
    assert_eq!(0, reader.position());
}

#[test]
fn test_mime_detector_backend_prefer_filename_skips_reader_and_file_content() {
    let detector = DirectBackendDetector::new();
    let mut reader = std::io::Cursor::new(b"xxxxx".to_vec());

    let from_reader = detector
        .detect_reader(
            &mut reader,
            Some("note.txt"),
            MimeDetectionPolicy::PreferFilename,
        )
        .expect("filename-preferred reader detection should succeed");

    let mut file = NamedTempFile::with_suffix(".txt")
        .expect("temp file should be created");
    std::io::Write::write_all(&mut file, b"xxxxx")
        .expect("temp file should be writable");
    let from_file = detector
        .detect_file(file.path(), MimeDetectionPolicy::PreferFilename)
        .expect("filename-preferred file detection should succeed");

    assert_eq!(Some("text/plain".to_owned()), from_reader);
    assert_eq!(Some("text/plain".to_owned()), from_file);
    assert_eq!(0, reader.position());
}

#[test]
fn test_default_mime_detector_returns_usable_detector() {
    let registry = MimeDetectorRegistry::builtin();
    let detector = registry
        .create_default(&MimeConfig::default())
        .expect("default detector");
    assert!(detector.detect_by_filename("document.pdf").is_some());
}

#[test]
fn test_mime_detector_registry_creates_boxed_and_shared_named_detectors() {
    let registry = MimeDetectorRegistry::builtin();
    let config = MimeConfig::default();
    let boxed = registry
        .create("repository", &config)
        .expect("repository boxed detector");
    let shared = registry
        .create("repository", &config)
        .expect("repository shared detector");
    let boxed_file = registry
        .create("file", &config)
        .expect("file boxed detector");
    let shared_file = registry
        .create("file", &config)
        .expect("file shared detector");

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
    assert!(registry.create("unknown", &config).is_err());
}

#[test]
fn test_mime_detector_registry_creates_from_explicit_registry() {
    let registry = MimeDetectorRegistry::builtin();
    let config = create_detector_config("repository");
    let boxed = registry
        .create("repository", &config)
        .expect("boxed registry selector should create detector");
    let shared = registry
        .create("repository", &config)
        .expect("shared registry selector should create detector");
    let shared_default = registry
        .create_default(&config)
        .expect("shared registry default should create detector");

    assert_eq!(
        Some("application/pdf".to_owned()),
        boxed.detect_by_filename("document.pdf")
    );
    assert_eq!(
        Some("application/pdf".to_owned()),
        shared.detect_by_filename("document.pdf")
    );
    assert_eq!(
        Some("application/pdf".to_owned()),
        shared_default.detect_by_filename("document.pdf")
    );
}

#[test]
fn test_boxed_mime_detector_trait_object_delegates_all_entry_points() {
    let detector: Box<dyn MimeDetector> = Box::new(StaticDetector);
    let mut reader = std::io::Cursor::new(b"data".to_vec());

    assert_eq!(
        Some("application/x-static-name".to_owned()),
        detector.detect_by_filename("file.bin")
    );
    assert_eq!(
        Some("application/x-static-content".to_owned()),
        detector.detect_by_content(b"data")
    );
    assert_eq!(
        Some("application/x-static-detect".to_owned()),
        detector.detect(
            b"data",
            Some("file.bin"),
            MimeDetectionPolicy::PreferFilename
        )
    );
    assert_eq!(
        Some("application/x-static-reader".to_owned()),
        detector
            .detect_reader(
                &mut reader,
                None,
                MimeDetectionPolicy::PreferFilename
            )
            .expect("boxed reader delegation should succeed")
    );
    assert_eq!(
        Some("application/x-static-file".to_owned()),
        detector
            .detect_file(
                std::path::Path::new("Cargo.toml"),
                MimeDetectionPolicy::PreferFilename
            )
            .expect("boxed file delegation should succeed")
    );
}

#[test]
fn test_shared_mime_detector_trait_object_delegates_all_entry_points() {
    let detector: std::sync::Arc<dyn MimeDetector> =
        std::sync::Arc::new(StaticDetector);
    let cloned = detector.clone();
    let mut reader = std::io::Cursor::new(b"data".to_vec());

    assert_eq!(
        Some("application/x-static-name".to_owned()),
        cloned.as_ref().detect_by_filename("file.bin")
    );
    assert_eq!(
        Some("application/x-static-content".to_owned()),
        detector.detect_by_content(b"data")
    );
    assert_eq!(
        Some("application/x-static-detect".to_owned()),
        detector.detect(
            b"data",
            Some("file.bin"),
            MimeDetectionPolicy::PreferFilename
        )
    );
    assert_eq!(
        Some("application/x-static-reader".to_owned()),
        detector
            .detect_reader(
                &mut reader,
                None,
                MimeDetectionPolicy::PreferFilename
            )
            .expect("arc reader delegation should succeed")
    );
    assert_eq!(
        Some("application/x-static-file".to_owned()),
        detector
            .detect_file(
                std::path::Path::new("Cargo.toml"),
                MimeDetectionPolicy::PreferFilename
            )
            .expect("arc file delegation should succeed")
    );
}

#[test]
fn test_mime_detector_registry_builds_from_config_defaults() {
    let registry = MimeDetectorRegistry::builtin();
    let config = MimeConfig::default();
    let file_config = create_detector_config("file");
    let unknown_config = create_detector_config("unknown");
    let fallback_config =
        create_detector_config_with_fallbacks("unknown", &["repository"]);
    let boxed = registry
        .create_default(&config)
        .expect("default boxed detector");
    let shared = registry
        .create_default(&config)
        .expect("default shared detector");
    let boxed_file = registry
        .create_default(&file_config)
        .expect("file boxed detector");
    let shared_file = registry
        .create_default(&file_config)
        .expect("file shared detector");
    let fallback = registry
        .create_default(&fallback_config)
        .expect("repository fallback detector");
    let repository_default = RepositoryMimeDetector::default();
    let file_default = FileCommandMimeDetector::default();
    let file_from_config =
        FileCommandMimeDetector::from_mime_config(file_config);

    assert!(boxed.detect_by_filename("document.pdf").is_some());
    assert!(shared.detect_by_filename("document.pdf").is_some());
    assert!(boxed_file.detect_by_filename("document.pdf").is_some());
    assert!(shared_file.detect_by_filename("document.pdf").is_some());
    assert!(fallback.detect_by_filename("document.pdf").is_some());
    assert!(registry.create_default(&unknown_config).is_err());
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
fn test_configured_fallback_uses_repository_after_unknown_detector() {
    let registry = MimeDetectorRegistry::builtin();
    let config =
        create_detector_config_with_fallbacks("unknown", &["repository"]);
    let detector = registry.create_default(&config).expect("fallback detector");

    assert_eq!(
        Some("application/pdf".to_owned()),
        detector.detect_by_filename("document.pdf")
    );
}

#[test]
fn test_detector_backend_error_builder_preserves_context() {
    let error = MimeError::detector_backend("custom-backend", "command failed");

    assert!(matches!(
        error,
        MimeError::DetectorBackend {
            ref backend,
            ref reason,
        } if backend == "custom-backend" && reason == "command failed"
    ));
    assert_eq!(
        "MIME detector backend 'custom-backend' failed: command failed",
        error.to_string()
    );
}

fn create_detector_config(detector: &str) -> MimeConfig {
    create_detector_config_with_fallbacks(detector, &[])
}

fn create_detector_config_with_fallbacks(
    detector: &str,
    fallbacks: &[&str],
) -> MimeConfig {
    let mut config = qubit_config::Config::new();
    config
        .set(qubit_mime::CONFIG_MIME_DETECTOR_DEFAULT, detector)
        .expect("detector default should be configurable");
    config
        .set(CONFIG_MIME_DETECTOR_FALLBACKS, fallbacks.join(","))
        .expect("detector fallbacks should be configurable");
    MimeConfig::from_config(&config).expect("detector config should parse")
}
