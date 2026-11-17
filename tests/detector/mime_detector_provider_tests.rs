// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_mime::AsyncMediaStreamClassifierProvider;
use qubit_mime::AsyncMimeDetectorProvider;
use qubit_mime::FfprobeCommandMediaStreamClassifierProvider;
use qubit_mime::FileCommandMimeDetectorProvider;
use qubit_mime::FixedMimeExecutionPort;
use qubit_mime::MimeCapability;
use qubit_mime::MimeConfig;
use qubit_mime::MimeError;
use qubit_mime::MimeExecutionServices;
use qubit_mime::RepositoryMimeDetectorProvider;
use qubit_spi::ProviderMetadata;
use qubit_spi::ServiceProvider;
use qubit_spi::error::ProviderFailureKind;

use crate::support::TestMimeDetectorProvider;
use crate::support::TestProviderBehavior;

#[test]
fn test_mime_detector_provider_defaults_and_factory() {
    let provider = TestMimeDetectorProvider::new("static", 7, TestProviderBehavior::Success("application/x-static"))
        .with_aliases(&["static-alias"]);
    let descriptor = provider.descriptor();
    let detector = provider
        .create_configured(&MimeConfig::default())
        .expect("static provider should create detector");

    assert_eq!("static", descriptor.id().as_str());
    assert_eq!("static-alias", descriptor.aliases()[0].as_str());
    assert_eq!(7, descriptor.priority());
    assert_eq!(
        Some("application/x-static".to_owned()),
        detector
            .detect_by_filename("sample.static")
            .expect("filename detection should succeed"),
    );
}

#[test]
fn test_mime_detector_provider_preserves_creation_failure_categories() {
    let unsupported = TestMimeDetectorProvider::new("unsupported", 0, TestProviderBehavior::Unsupported)
        .create_configured(&MimeConfig::default())
        .expect_err("unsupported providers must not create a detector");
    assert_eq!(ProviderFailureKind::Unsupported, unsupported.kind());
    assert!(unsupported.to_string().contains("unsupported input"));

    let unavailable = TestMimeDetectorProvider::new("unavailable", 0, TestProviderBehavior::Unavailable)
        .create_configured(&MimeConfig::default())
        .expect_err("unavailable providers must report their missing dependency");
    assert_eq!(ProviderFailureKind::Unavailable, unavailable.kind());
    assert!(unavailable.to_string().contains("missing executable"));
}

#[test]
fn test_async_detector_providers_handle_classifier_capability_explicitly() {
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let group = manager.group(qubit_operation::GroupOptions { capacity: 2 }).unwrap();
    let services = MimeExecutionServices::new(
        manager,
        group,
        std::sync::Arc::new(FixedMimeExecutionPort::new(1, 2)),
        1024,
    )
    .expect("valid execution services should be created");
    let config = MimeConfig::default();
    let classifier = FfprobeCommandMediaStreamClassifierProvider
        .create_async_backend(&config, &services)
        .expect("classifier backend construction should not probe for ffprobe");

    let repository = RepositoryMimeDetectorProvider
        .create_async_backend_with_classifier(&config, &services, Some(classifier.clone()))
        .expect("repository provider supports an async classifier");
    match repository.content_requirement() {
        qubit_mime::ContentRequirement::Prefix(limit) => assert!(limit > 0),
        qubit_mime::ContentRequirement::Complete => {
            panic!("repository backend should declare its bounded prefix requirement")
        }
    }
    assert_eq!(
        qubit_mime::RandomAccessCapability::Unsupported,
        repository.random_access_capability()
    );

    let error = FileCommandMimeDetectorProvider
        .create_async_backend_with_classifier(&config, &services, Some(classifier))
        .expect_err("file-command provider cannot attach a same-object classifier");
    assert!(matches!(
        error,
        MimeError::UnsupportedCapability {
            capability: MimeCapability::SameObjectRefinement
        }
    ));
}
