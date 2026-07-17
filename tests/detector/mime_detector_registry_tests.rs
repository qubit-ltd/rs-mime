// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::process::Command;
use std::sync::Arc;

use qubit_mime::{
    MimeConfig,
    MimeDetector,
    MimeDetectorRegistry,
    MimeDetectorSpec,
};
use qubit_spi::error::{
    ProviderCreationError,
    ProviderErrorKind,
    ProviderSelectionError,
};
use qubit_spi::{
    FallbackPolicy,
    ProviderCreationTermination,
    ProviderDefinition,
    ProviderSelection,
    ServiceProvider,
};

use crate::support::{
    TestMimeDetectorProvider,
    TestProviderBehavior,
};

#[test]
fn test_builtin_registry_lists_and_resolves_repository_provider() {
    let registry = MimeDetectorRegistry::builtin();
    let expected_default = ProviderSelection::named("repository")
        .expect("repository provider ID should be valid");
    let selection = ProviderSelection::named("repository-mime-detector")
        .expect("repository alias should be valid");
    let detector = registry
        .resolve(&selection)
        .expect("repository alias should resolve")
        .create_default()
        .expect("repository alias should resolve");

    assert_eq!(
        vec!["repository", "file"],
        registry
            .provider_ids()
            .iter()
            .map(|id| id.as_str())
            .collect::<Vec<_>>(),
    );
    assert_eq!(expected_default, registry.default_selection());
    assert_eq!(
        Some("application/pdf".to_owned()),
        detector.detect_by_filename("document.pdf"),
    );
}

#[test]
fn test_global_registry_exposes_builtin_defaults_in_this_process() {
    let registry = MimeDetectorRegistry::global();

    assert!(
        registry
            .provider_ids()
            .iter()
            .any(|id| id.as_str() == "repository"),
    );
    assert_eq!(
        ProviderSelection::named("repository")
            .expect("repository provider ID should be valid"),
        registry.default_selection(),
    );
}

#[test]
fn test_builder_registers_owned_and_shared_providers_atomically() {
    let mut builder = MimeDetectorRegistry::builder();
    builder
        .register(TestMimeDetectorProvider::new(
            "owned",
            20,
            TestProviderBehavior::Success("application/x-owned"),
        ))
        .expect("owned provider should register");
    let shared: Arc<dyn ProviderDefinition<MimeDetectorSpec>> =
        Arc::new(TestMimeDetectorProvider::new(
            "shared",
            10,
            TestProviderBehavior::Success("application/x-shared"),
        ));
    builder
        .register_shared(shared)
        .expect("shared provider should register");

    let duplicate = builder
        .register(TestMimeDetectorProvider::new(
            "shared",
            0,
            TestProviderBehavior::Success("application/x-duplicate"),
        ))
        .expect_err("duplicate selector should be rejected");
    assert!(duplicate.to_string().contains("shared"));

    let registry = builder.build();
    let runtime_shared: Arc<dyn ProviderDefinition<MimeDetectorSpec>> =
        Arc::new(TestMimeDetectorProvider::new(
            "runtime-shared",
            0,
            TestProviderBehavior::Success("application/x-runtime-shared"),
        ));
    registry
        .register_shared(runtime_shared)
        .expect("runtime shared provider should register");

    assert_eq!(3, registry.provider_ids().len());
}

#[test]
fn test_resolve_reports_selection_errors_before_creation() {
    let registry = MimeDetectorRegistry::builder().build();
    let missing = ProviderSelection::named("missing")
        .expect("missing selector should still be syntactically valid");

    assert!(matches!(
        registry.resolve(&missing),
        Err(ProviderSelectionError::UnknownProvider { selector, .. })
            if selector.as_str() == "missing"
    ));
    assert!(matches!(
        registry.resolve(&ProviderSelection::auto()),
        Err(ProviderSelectionError::EmptyRegistry)
    ));
}

#[test]
fn test_resolve_and_create_keep_selection_and_creation_errors_separate() {
    let registry = MimeDetectorRegistry::builder().build();
    registry
        .register(TestMimeDetectorProvider::new(
            "failed",
            0,
            TestProviderBehavior::InitializationFailed,
        ))
        .expect("failing provider should register");
    let selection = ProviderSelection::named("failed")
        .expect("test selector should be valid");
    let provider = registry
        .resolve(&selection)
        .expect("provider selection should succeed before creation");
    let error = provider
        .create_default()
        .expect_err("selected provider should fail during creation");

    assert!(matches!(
        error,
        ProviderCreationError::NoProviderSucceeded { .. }
    ));
    let attempt = error
        .decisive_attempt()
        .expect("one failed provider should be decisive");
    assert_eq!("failed", attempt.provider_id().as_str());
    assert_eq!(
        ProviderErrorKind::InitializationFailed,
        attempt.error().kind()
    );
}

#[test]
fn test_default_selection_is_independent_from_service_configuration() {
    let mut builder = MimeDetectorRegistry::builder();
    builder
        .register(TestMimeDetectorProvider::new(
            "configured",
            0,
            TestProviderBehavior::Success("application/x-configured"),
        ))
        .expect("configured provider should register");
    let registry = builder.build();
    registry.set_default_selection(
        ProviderSelection::named("configured")
            .expect("configured selector should be valid"),
    );

    let provider = registry
        .resolve_default()
        .expect("registry default should resolve without MIME config");
    let config = MimeConfig::default();
    let explicit = provider
        .create(&config)
        .expect("explicit MIME config should create a detector");
    let defaulted = provider
        .create_default()
        .expect("default MIME config should create a detector");

    assert_eq!(
        Some("application/x-configured".to_owned()),
        explicit.detect_by_filename("sample.static"),
    );
    assert_eq!(
        Some("application/x-configured".to_owned()),
        defaulted.detect_by_filename("sample.static"),
    );
}

#[test]
fn test_resolving_provider_applies_selection_fallback_policy() {
    let mut builder = MimeDetectorRegistry::builder();
    builder
        .register(TestMimeDetectorProvider::new(
            "unavailable",
            20,
            TestProviderBehavior::Unavailable,
        ))
        .expect("unavailable provider should register");
    builder
        .register(TestMimeDetectorProvider::new(
            "success",
            10,
            TestProviderBehavior::Success("application/x-static"),
        ))
        .expect("successful provider should register");
    let registry = builder.build();
    let selection = ProviderSelection::chain(["unavailable", "success"])
        .expect("test chain should be valid")
        .with_fallback_policy(FallbackPolicy::OnAbsence);
    let detector = registry
        .resolve(&selection)
        .expect("both candidates should resolve")
        .create_default()
        .expect("absence fallback should reach the successful provider");

    assert_eq!(
        Some("application/x-static".to_owned()),
        detector.detect_by_filename("sample.static"),
    );
}

#[test]
fn test_resolving_provider_reports_policy_stop_with_actual_attempts() {
    let mut builder = MimeDetectorRegistry::builder();
    builder
        .register(TestMimeDetectorProvider::new(
            "terminal",
            10,
            TestProviderBehavior::InitializationFailed,
        ))
        .expect("terminal provider should register");
    builder
        .register(TestMimeDetectorProvider::new(
            "unreached",
            0,
            TestProviderBehavior::Success("application/x-unreached"),
        ))
        .expect("unreached provider should register");
    let selection = ProviderSelection::chain(["terminal", "unreached"])
        .expect("test chain should be valid")
        .with_fallback_policy(FallbackPolicy::OnAbsence);
    let error = builder
        .build()
        .resolve(&selection)
        .expect("both providers should resolve")
        .create_default()
        .expect_err("initialization failure should stop absence fallback");

    assert_eq!(
        Some(ProviderCreationTermination::StoppedByPolicy),
        error.termination(),
    );
    assert_eq!(1, error.attempts().len());
    assert_eq!("terminal", error.attempts()[0].provider_id().as_str());
}

/// Simulates library X resolving and creating a default detector without
/// knowing the App-selected implementation.
///
/// # Returns
///
/// The detector created through the process-wide Registry defaults.
fn library_x_create_default_detector() -> Arc<dyn MimeDetector> {
    MimeDetectorRegistry::global()
        .resolve_default()
        .expect("App-configured global provider should resolve")
        .create_default()
        .expect("App-configured global provider should create a detector")
}

#[test]
fn test_global_registry_shares_app_provider_with_library_x() {
    const CHILD_MARKER: &str = "QUBIT_MIME_TEST_GLOBAL_PROVIDER_REGISTRY";
    const PROVIDER_ID: &str = "app-global-static-detector";

    if std::env::var_os(CHILD_MARKER).is_some() {
        let registry = MimeDetectorRegistry::global();
        registry
            .register(TestMimeDetectorProvider::new(
                PROVIDER_ID,
                100,
                TestProviderBehavior::Success("application/x-app-global"),
            ))
            .expect("App provider should register globally");
        let selection = ProviderSelection::named(PROVIDER_ID)
            .expect("App provider selector should be valid");

        let explicit = registry
            .resolve(&selection)
            .expect("explicit App selection should resolve")
            .create(&MimeConfig::default())
            .expect("explicit MIME config should create the App detector");
        registry.set_default_selection(selection);
        let defaulted = library_x_create_default_detector();

        assert_eq!(
            Some("application/x-app-global".to_owned()),
            explicit.detect_by_filename("sample.static"),
        );
        assert_eq!(
            Some("application/x-app-global".to_owned()),
            defaulted.detect_by_filename("sample.static"),
        );
        return;
    }

    let current_test = std::env::current_exe()
        .expect("current integration test executable should be available");
    let test_name = "detector::mime_detector_registry_tests::test_global_registry_shares_app_provider_with_library_x";
    let status = Command::new(current_test)
        .args(["--exact", test_name, "--nocapture"])
        .env(CHILD_MARKER, "1")
        .status()
        .expect("isolated global Registry scenario should start");

    assert!(status.success(), "isolated global Registry scenario failed");
}
