// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_local_files::LocalTempFile as TempFile;
use qubit_mime::{
    CONFIG_MIME_DETECTOR_FALLBACKS,
    FileCommandMimeDetector,
    MimeConfig,
    MimeDetectionPolicy,
    MimeDetector,
    MimeDetectorBackend,
    MimeDetectorRegistry,
    MimeError,
    RepositoryMimeDetector,
};
use qubit_spi::ProviderSelection;

#[cfg(unix)]
use crate::support::PathEnvGuard;
use crate::support::{
    DirectBackendDetector,
    StaticEntryPointMimeDetector,
};

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

    let mut file =
        TempFile::with_suffix(".pdf").expect("temp file should be created");
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

    let mut file = TempFile::new().expect("temp file should be created");
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

    let mut file =
        TempFile::with_suffix(".txt").expect("temp file should be created");
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
        .resolve()
        .expect("default provider selection")
        .create()
        .expect("default detector");
    assert!(detector.detect_by_filename("document.pdf").is_some());
}

#[test]
fn test_mime_detector_registry_creates_boxed_and_shared_named_detectors() {
    #[cfg(unix)]
    let _path_guard = PathEnvGuard::preserve();

    let registry = MimeDetectorRegistry::builtin();
    let config = MimeConfig::default();
    let boxed = create_named_detector(&registry, "repository", &config);
    let shared = create_named_detector(&registry, "repository", &config);
    let boxed_file = create_named_detector(&registry, "file", &config);
    let shared_file = create_named_detector(&registry, "file", &config);

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
    let unknown = ProviderSelection::named("unknown")
        .expect("unknown selector should still be syntactically valid");
    assert!(registry.resolve_selected(&unknown).is_err());
}

#[test]
fn test_mime_detector_registry_creates_from_explicit_registry() {
    let registry = MimeDetectorRegistry::builtin();
    let config = create_detector_config("repository");
    let boxed = create_named_detector(&registry, "repository", &config);
    let shared = create_named_detector(&registry, "repository", &config);
    let shared_default = registry
        .resolve_selected(config.mime_detector_selection())
        .expect("configured registry selection should resolve")
        .create_configured(&config)
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
    let detector: Box<dyn MimeDetector> =
        Box::new(StaticEntryPointMimeDetector);
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
        std::sync::Arc::new(StaticEntryPointMimeDetector);
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
    let boxed = create_configured_detector(&registry, &config);
    let shared = create_configured_detector(&registry, &config);
    let boxed_file = create_configured_detector(&registry, &file_config);
    let shared_file = create_configured_detector(&registry, &file_config);
    let repository_default = RepositoryMimeDetector::default();
    let file_default = FileCommandMimeDetector::default();
    let file_from_config =
        FileCommandMimeDetector::from_mime_config(file_config);

    assert!(boxed.detect_by_filename("document.pdf").is_some());
    assert!(shared.detect_by_filename("document.pdf").is_some());
    assert!(boxed_file.detect_by_filename("document.pdf").is_some());
    assert!(shared_file.detect_by_filename("document.pdf").is_some());
    assert!(
        registry
            .resolve_selected(unknown_config.mime_detector_selection())
            .is_err()
    );
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
fn test_configured_fallback_rejects_unknown_detector() {
    let registry = MimeDetectorRegistry::builtin();
    let config =
        create_detector_config_with_fallbacks("unknown", &["repository"]);
    let error = registry
        .resolve_selected(config.mime_detector_selection())
        .expect_err("strict configured chain should reject unknown providers");

    assert!(error.is_unknown_providers());
    assert!(
        error
            .selectors()
            .expect("unknown-provider errors should retain selectors")
            .iter()
            .any(|selector| selector.as_str() == "unknown")
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

/// Resolves a named provider and creates its detector with explicit config.
///
/// # Parameters
///
/// * `registry` - Registry containing the named provider.
/// * `selector` - Canonical ID or alias to resolve.
/// * `config` - MIME configuration supplied only during service creation.
///
/// # Returns
///
/// The detector created by the selected provider.
fn create_named_detector(
    registry: &MimeDetectorRegistry,
    selector: &str,
    config: &MimeConfig,
) -> std::sync::Arc<dyn MimeDetector> {
    let selection = ProviderSelection::named(selector)
        .expect("test provider selector should be valid");
    registry
        .resolve_selected(&selection)
        .expect("named detector provider should resolve")
        .create_configured(config)
        .expect("named detector provider should create its service")
}

/// Resolves the optional selection carried by a MIME configuration object.
///
/// # Parameters
///
/// * `registry` - Registry containing configured candidates.
/// * `config` - Independent source of both a selection and service settings.
///
/// # Returns
///
/// The detector created after the explicit two-stage operation.
fn create_configured_detector(
    registry: &MimeDetectorRegistry,
    config: &MimeConfig,
) -> std::sync::Arc<dyn MimeDetector> {
    registry
        .resolve_selected(config.mime_detector_selection())
        .expect("configured detector selection should resolve")
        .create_configured(config)
        .expect("configured detector provider should create its service")
}
