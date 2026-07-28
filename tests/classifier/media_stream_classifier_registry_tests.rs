// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::{
    process::Command,
    sync::Arc,
};

#[cfg(unix)]
use qubit_local_files::LocalTempDirectory as TempDir;
use qubit_mime::{
    MediaStreamClassifierRegistry,
    MediaStreamClassifierSpec,
    MimeConfig,
};
use qubit_spi::error::ProviderFailureKind;
use qubit_spi::{
    FallbackPolicy,
    ProviderCreationTermination,
    ProviderDefinition,
    ProviderSelection,
};

#[cfg(unix)]
use crate::support::PathEnvGuard;
use crate::support::{
    TestMediaStreamClassifierProvider,
    TestProviderBehavior,
};

const FFPROBE_MISSING_CHILD: &str = "QUBIT_MIME_FFPROBE_MISSING_CHILD";

/// Verifies a missing FFprobe executable is classified as unavailable.
#[test]
fn test_ffprobe_provider_reports_unavailable_when_command_is_missing() {
    if std::env::var_os(FFPROBE_MISSING_CHILD).is_some() {
        let registry = MediaStreamClassifierRegistry::builtin();
        let selection = ProviderSelection::named("ffprobe")
            .expect("FFprobe provider ID should be valid");
        let error = registry
            .resolve_selected(&selection)
            .expect("FFprobe provider should resolve")
            .create()
            .expect_err("missing FFprobe should fail during creation");
        let attempt = error.decisive_attempt();

        assert_eq!(ProviderFailureKind::Unavailable, attempt.failure().kind());
        return;
    }

    let test_binary = std::env::current_exe()
        .expect("the integration-test executable should have a path");
    let status = Command::new(test_binary)
        .arg("--exact")
        .arg(concat!(
            "classifier::media_stream_classifier_registry_tests::",
            "test_ffprobe_provider_reports_unavailable_when_command_is_missing",
        ))
        .arg("--nocapture")
        .env(FFPROBE_MISSING_CHILD, "1")
        .env("PATH", "")
        .status()
        .expect("isolated missing-FFprobe test process should start");

    assert!(status.success(), "isolated missing-FFprobe test failed");
}

/// Verifies the FFprobe provider creates a classifier when FFprobe is present.
#[test]
#[cfg(unix)]
fn test_ffprobe_provider_creates_classifier_when_command_is_available() {
    use std::os::unix::fs::PermissionsExt;

    let temp_dir =
        TempDir::new().expect("temporary command directory should be created");
    let script_path = temp_dir.path().join("ffprobe");
    std::fs::write(&script_path, "#!/bin/sh\nexit 0\n")
        .expect("fake FFprobe should be written");
    let mut permissions = std::fs::metadata(&script_path)
        .expect("fake FFprobe metadata should be readable")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&script_path, permissions)
        .expect("fake FFprobe should be executable");
    let _path_guard = PathEnvGuard::set(temp_dir.path());

    let selection = ProviderSelection::named("ffprobe")
        .expect("FFprobe provider ID should be valid");
    MediaStreamClassifierRegistry::builtin()
        .resolve_selected(&selection)
        .expect("FFprobe provider should resolve")
        .create()
        .expect("available FFprobe should create a classifier");
}

#[test]
fn test_builtin_registry_lists_and_resolves_ffprobe_provider() {
    let registry = MediaStreamClassifierRegistry::builtin();
    let expected_default = ProviderSelection::named("ffprobe")
        .expect("FFprobe provider ID should be valid");
    let selection = ProviderSelection::named("ffprobe-command")
        .expect("FFprobe alias should be valid");

    let alias_creation = registry
        .resolve_selected(&selection)
        .expect("FFprobe alias should resolve")
        .create();
    let default_creation = registry
        .resolve()
        .expect("built-in classifier default should resolve")
        .create_configured(&MimeConfig::default());
    for creation in [alias_creation, default_creation] {
        if let Err(error) = creation {
            assert_eq!(
                ProviderFailureKind::Unavailable,
                error.decisive_attempt().failure().kind(),
            );
        }
    }
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
    assert_eq!("shared", duplicate.selector());

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

    let unknown = registry
        .resolve_selected(&missing)
        .expect_err("missing provider should fail resolution");
    assert!(unknown.is_unknown_providers());
    let selectors = unknown
        .selectors()
        .expect("unknown-provider errors should retain selectors");
    assert_eq!(1, selectors.len());
    assert_eq!("missing", selectors[0].as_str());

    let empty = registry
        .resolve_selected(&ProviderSelection::auto())
        .expect_err("automatic selection from an empty Registry should fail");
    assert!(empty.is_empty_registry());
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

    let attempt = error.decisive_attempt();
    assert_eq!("failed", attempt.provider_id().as_str());
    assert_eq!(
        ProviderFailureKind::InitializationFailed,
        attempt.failure().kind()
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
        ProviderCreationTermination::StoppedByPolicy,
        error.termination(),
    );
    assert_eq!(1, error.attempts().len());
}
