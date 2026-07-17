// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::Arc;

use qubit_mime::{
    MediaStreamClassifierRegistry,
    MediaStreamClassifierSpec,
    MimeConfig,
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
    TestMediaStreamClassifierProvider,
    TestProviderBehavior,
};

#[test]
fn test_builtin_registry_lists_and_resolves_ffprobe_provider() {
    let registry = MediaStreamClassifierRegistry::builtin();
    let expected_default = ProviderSelection::named("ffprobe")
        .expect("FFprobe provider ID should be valid");
    let selection = ProviderSelection::named("ffprobe-command")
        .expect("FFprobe alias should be valid");

    registry
        .resolve_selected(&selection)
        .expect("FFprobe alias should resolve")
        .create()
        .expect("FFprobe alias should resolve");
    registry
        .resolve()
        .expect("built-in classifier default should resolve")
        .create_configured(&MimeConfig::default())
        .expect("explicit MIME config should create FFprobe classifier");
    assert_eq!(
        vec!["ffprobe"],
        registry
            .provider_ids()
            .iter()
            .map(|id| id.as_str())
            .collect::<Vec<_>>(),
    );
    assert_eq!(expected_default, registry.default_selection());
}

#[test]
fn test_global_classifier_registry_exposes_builtin_defaults() {
    let registry = MediaStreamClassifierRegistry::global();

    assert!(
        registry
            .provider_ids()
            .iter()
            .any(|id| id.as_str() == "ffprobe"),
    );
    assert_eq!(
        ProviderSelection::named("ffprobe")
            .expect("FFprobe provider ID should be valid"),
        registry.default_selection(),
    );
}

#[test]
fn test_registry_registers_owned_and_shared_providers_atomically() {
    let registry = MediaStreamClassifierRegistry::default();
    registry
        .register(TestMediaStreamClassifierProvider::new(
            "owned",
            20,
            TestProviderBehavior::Success("unused"),
        ))
        .expect("owned provider should register");
    let shared: Arc<dyn ProviderDefinition<MediaStreamClassifierSpec>> =
        Arc::new(TestMediaStreamClassifierProvider::new(
            "shared",
            10,
            TestProviderBehavior::Success("unused"),
        ));
    registry
        .register_shared(shared)
        .expect("shared provider should register");

    let duplicate = registry
        .register(TestMediaStreamClassifierProvider::new(
            "shared",
            0,
            TestProviderBehavior::Success("unused"),
        ))
        .expect_err("duplicate selector should be rejected");
    assert!(duplicate.to_string().contains("shared"));

    let runtime_shared: Arc<dyn ProviderDefinition<MediaStreamClassifierSpec>> =
        Arc::new(TestMediaStreamClassifierProvider::new(
            "runtime-shared",
            0,
            TestProviderBehavior::Success("unused"),
        ));
    registry
        .register_shared(runtime_shared)
        .expect("runtime shared provider should register");

    assert_eq!(3, registry.provider_ids().len());
}

#[test]
fn test_resolve_reports_classifier_selection_errors_before_creation() {
    let registry = MediaStreamClassifierRegistry::default();
    let missing = ProviderSelection::named("missing")
        .expect("missing selector should still be syntactically valid");

    assert!(matches!(
        registry.resolve_selected(&missing),
        Err(ProviderSelectionError::UnknownProvider { selector, .. })
            if selector.as_str() == "missing"
    ));
    assert!(matches!(
        registry.resolve_selected(&ProviderSelection::auto()),
        Err(ProviderSelectionError::EmptyRegistry)
    ));
}

#[test]
fn test_resolve_and_create_keep_classifier_errors_separate() {
    let registry = MediaStreamClassifierRegistry::default();
    registry
        .register(TestMediaStreamClassifierProvider::new(
            "failed",
            0,
            TestProviderBehavior::InitializationFailed,
        ))
        .expect("failing provider should register");
    let selection = ProviderSelection::named("failed")
        .expect("test selector should be valid");
    let provider = registry
        .resolve_selected(&selection)
        .expect("selection should succeed before creation");
    let error = provider
        .create()
        .expect_err("selected classifier should fail during creation");

    assert!(matches!(
        error,
        ProviderCreationError::NoProviderSucceeded { .. }
    ));
    let attempt = error
        .decisive_attempt()
        .expect("one failed classifier should be decisive");
    assert_eq!("failed", attempt.provider_id().as_str());
    assert_eq!(
        ProviderErrorKind::InitializationFailed,
        attempt.error().kind()
    );
}

#[test]
fn test_classifier_default_selection_is_independent_from_mime_config() {
    let registry = MediaStreamClassifierRegistry::default();
    registry
        .register(TestMediaStreamClassifierProvider::new(
            "configured",
            10,
            TestProviderBehavior::Success("unused"),
        ))
        .expect("successful provider should register");
    registry.set_default_selection(
        ProviderSelection::named("configured")
            .expect("configured selector should be valid"),
    );
    let provider = registry
        .resolve()
        .expect("default selection should resolve without MIME config");

    provider
        .create_configured(&MimeConfig::default())
        .expect("explicit MIME config should create the classifier");
    provider
        .create()
        .expect("default MIME config should create the classifier");
}

#[test]
fn test_classifier_resolving_provider_applies_fallback_policy() {
    let registry = MediaStreamClassifierRegistry::default();
    registry
        .register(TestMediaStreamClassifierProvider::new(
            "unsupported",
            30,
            TestProviderBehavior::Unsupported,
        ))
        .expect("unsupported provider should register");
    registry
        .register(TestMediaStreamClassifierProvider::new(
            "success",
            10,
            TestProviderBehavior::Success("unused"),
        ))
        .expect("successful provider should register");
    let selection = ProviderSelection::chain(["unsupported", "success"])
        .expect("test chain should be valid")
        .with_fallback_policy(FallbackPolicy::OnAbsence);

    registry
        .resolve_selected(&selection)
        .expect("both classifiers should resolve")
        .create()
        .expect("absence fallback should reach the successful classifier");
}

#[test]
fn test_classifier_creation_reports_policy_stop() {
    let registry = MediaStreamClassifierRegistry::default();
    registry
        .register(TestMediaStreamClassifierProvider::new(
            "terminal",
            20,
            TestProviderBehavior::InitializationFailed,
        ))
        .expect("terminal provider should register");
    registry
        .register(TestMediaStreamClassifierProvider::new(
            "unreached",
            10,
            TestProviderBehavior::Success("unused"),
        ))
        .expect("unreached provider should register");
    let selection = ProviderSelection::chain(["terminal", "unreached"])
        .expect("test chain should be valid")
        .with_fallback_policy(FallbackPolicy::OnAbsence);
    let error = registry
        .resolve_selected(&selection)
        .expect("both classifiers should resolve")
        .create()
        .expect_err("initialization failure should stop absence fallback");

    assert_eq!(
        Some(ProviderCreationTermination::StoppedByPolicy),
        error.termination(),
    );
    assert_eq!(1, error.attempts().len());
}
