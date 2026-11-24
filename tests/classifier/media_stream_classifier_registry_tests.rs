// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::time::Duration;

#[cfg(unix)]
use qubit_local_files::LocalFileSystem;
#[cfg(unix)]
use qubit_local_files::options::LocalTempDirectoryOptions;
use qubit_mime::AsyncMediaStreamClassifierProvider;
use qubit_mime::FfprobeCommandMediaStreamClassifierProvider;
use qubit_mime::FixedMimeExecutionPort;
use qubit_mime::MediaStreamClassifierRegistry;
use qubit_mime::MediaStreamClassifierSpec;
use qubit_mime::MimeConfig;
use qubit_mime::MimeError;
use qubit_mime::MimeExecutionServices;
use qubit_operation::GroupOptions;
use qubit_operation::OperationManager;
use qubit_spi::FallbackPolicy;
use qubit_spi::ProviderCreationTermination;
use qubit_spi::ProviderDefinition;
use qubit_spi::ProviderDescriptor;
use qubit_spi::ProviderId;
use qubit_spi::ProviderMetadata;
use qubit_spi::ProviderSelection;
use qubit_spi::error::ProviderFailureKind;

#[cfg(unix)]
use crate::support::PathEnvGuard;
use crate::support::TestMediaStreamClassifierProvider;
use crate::support::TestProviderBehavior;

const FFPROBE_MISSING_CHILD: &str = "QUBIT_MIME_FFPROBE_MISSING_CHILD";

/// Verifies a missing FFprobe executable is classified as unavailable.
#[test]
fn test_ffprobe_provider_reports_unavailable_when_command_is_missing() {
    if std::env::var_os(FFPROBE_MISSING_CHILD).is_some() {
        let registry = MediaStreamClassifierRegistry::builtin();
        let selection = ProviderSelection::named("ffprobe").expect("FFprobe provider ID should be valid");
        let error = registry
            .resolve_selected(&selection)
            .expect("FFprobe provider should resolve")
            .create()
            .expect_err("missing FFprobe should fail during creation");
        let attempt = error.decisive_attempt();

        assert_eq!(ProviderFailureKind::Unavailable, attempt.failure().kind());
        return;
    }

    let test_binary = std::env::current_exe().expect("the integration-test executable should have a path");
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
    let temp_dir = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .create_temp_directory_with_options(&LocalTempDirectoryOptions::new())
        .expect("temporary command directory should be created");
    let script_path = temp_dir.path().join("ffprobe");
    std::fs::write(&script_path, "#!/bin/sh\nexit 0\n").expect("fake FFprobe should be written");
    let mut permissions = std::fs::metadata(&script_path)
        .expect("fake FFprobe metadata should be readable")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&script_path, permissions).expect("fake FFprobe should be executable");
    let _path_guard = PathEnvGuard::set(temp_dir.path());

    let selection = ProviderSelection::named("ffprobe").expect("FFprobe provider ID should be valid");
    MediaStreamClassifierRegistry::builtin()
        .resolve_selected(&selection)
        .expect("FFprobe provider should resolve")
        .create()
        .expect("available FFprobe should create a classifier");
}

#[test]
fn test_builtin_registry_lists_and_resolves_ffprobe_provider() {
    #[cfg(unix)]
    let _path_guard = PathEnvGuard::preserve();
    let registry = MediaStreamClassifierRegistry::builtin();
    let expected_default = ProviderSelection::named("ffprobe").expect("FFprobe provider ID should be valid");
    let selection = ProviderSelection::named("ffprobe-command").expect("FFprobe alias should be valid");

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
        registry.provider_ids().iter().map(|id| id.as_str()).collect::<Vec<_>>(),
    );
    assert_eq!(expected_default, registry.default_selection());
}

#[test]
fn test_global_classifier_registry_exposes_builtin_defaults() {
    let registry = MediaStreamClassifierRegistry::global();

    assert!(registry.provider_ids().iter().any(|id| id.as_str() == "ffprobe"),);
    assert_eq!(
        ProviderSelection::named("ffprobe").expect("FFprobe provider ID should be valid"),
        registry.default_selection(),
    );
}

#[test]
fn test_classifier_registry_seal_prevents_further_mutation() {
    let registry = MediaStreamClassifierRegistry::builtin();
    assert!(!registry.is_sealed());

    registry.seal();

    assert!(registry.is_sealed());
    assert!(
        registry
            .register(TestMediaStreamClassifierProvider::new(
                "after-seal",
                0,
                TestProviderBehavior::Success("unused"),
            ))
            .is_err()
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
    let shared: Arc<dyn ProviderDefinition<MediaStreamClassifierSpec>> = Arc::new(
        TestMediaStreamClassifierProvider::new("shared", 10, TestProviderBehavior::Success("unused")),
    );
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
    assert_eq!(Some("shared"), duplicate.selector());

    let runtime_shared: Arc<dyn ProviderDefinition<MediaStreamClassifierSpec>> = Arc::new(
        TestMediaStreamClassifierProvider::new("runtime-shared", 0, TestProviderBehavior::Success("unused")),
    );
    registry
        .register_shared(runtime_shared)
        .expect("runtime shared provider should register");

    assert_eq!(3, registry.provider_ids().len());
}

#[test]
fn test_resolve_reports_classifier_selection_errors_before_creation() {
    let registry = MediaStreamClassifierRegistry::default();
    let missing = ProviderSelection::named("missing").expect("missing selector should still be syntactically valid");

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
    let selection = ProviderSelection::named("failed").expect("test selector should be valid");
    let provider = registry
        .resolve_selected(&selection)
        .expect("selection should succeed before creation");
    let error = provider
        .create()
        .expect_err("selected classifier should fail during creation");

    let attempt = error.decisive_attempt();
    assert_eq!("failed", attempt.provider_id().as_str());
    assert_eq!(ProviderFailureKind::InitializationFailed, attempt.failure().kind());
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
    let _ = registry
        .set_default_selection(ProviderSelection::named("configured").expect("configured selector should be valid"));
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

    assert_eq!(ProviderCreationTermination::StoppedByPolicy, error.termination(),);
    assert_eq!(1, error.attempts().len());
}

#[test]
fn test_async_registry_resolves_auto_alias_chain_and_unknown_selection() {
    let manager = OperationManager::new(Default::default()).unwrap();
    let group = manager.group(GroupOptions { capacity: 2 }).unwrap();
    let services = MimeExecutionServices::new(manager, group, Arc::new(FixedMimeExecutionPort::new(1, 2)), 32).unwrap();
    let registry = MediaStreamClassifierRegistry::builtin();
    let config = MimeConfig::default();
    assert_eq!(
        vec!["ffprobe"],
        registry
            .async_provider_ids()
            .iter()
            .map(|id| id.as_str())
            .collect::<Vec<_>>(),
    );

    for selection in [
        ProviderSelection::auto(),
        ProviderSelection::named("ffprobe-command").unwrap(),
        ProviderSelection::chain_allowing_missing(["not-registered", "ffprobe-command"])
            .expect("the fallback selector chain should be valid"),
    ] {
        let (id, _backend) = registry
            .create_async_backend(&selection, &config, &services)
            .expect("the built-in async classifier should resolve its default and aliases");
        assert_eq!("ffprobe", id);
    }

    let error = registry
        .create_async_backend(&ProviderSelection::named("not-registered").unwrap(), &config, &services)
        .expect_err("unknown async selectors have no backend");
    assert!(error.to_string().contains("not-registered"));

    let strict_chain = ProviderSelection::chain(["not-registered", "ffprobe-command"])
        .expect("strict chain selectors should be valid");
    assert!(
        registry
            .create_async_backend(&strict_chain, &config, &services)
            .is_err(),
        "strict chains should diagnose missing selectors instead of skipping them"
    );

    let empty = MediaStreamClassifierRegistry::default();
    assert!(empty.async_provider_ids().is_empty());
    assert!(
        empty
            .create_async_backend(&ProviderSelection::auto(), &config, &services)
            .expect_err("empty async catalog should have a selection diagnostic")
            .to_string()
            .contains("no media stream classifier candidates")
    );
}

#[test]
fn test_async_classifier_chain_treats_known_sync_only_selector_as_absence() {
    let registry = MediaStreamClassifierRegistry::builtin();
    registry
        .register(TestMediaStreamClassifierProvider::new(
            "sync-only-classifier",
            100,
            TestProviderBehavior::Success("unused"),
        ))
        .unwrap();
    let manager = OperationManager::new(Default::default()).unwrap();
    let group = manager.group(GroupOptions { capacity: 2 }).unwrap();
    let services = MimeExecutionServices::new(manager, group, Arc::new(FixedMimeExecutionPort::new(1, 2)), 32).unwrap();
    let selection = ProviderSelection::chain(["sync-only-classifier", "ffprobe"])
        .unwrap()
        .with_fallback_policy(FallbackPolicy::OnAbsence);

    let (id, _) = registry
        .create_async_backend(&selection, &MimeConfig::default(), &services)
        .expect("known sync-only selector should allow fallback to FFprobe");

    assert_eq!("ffprobe", id);
}

#[test]
fn test_async_registry_rejects_duplicate_provider_registration() {
    let registry = MediaStreamClassifierRegistry::builtin();
    let error = registry
        .register_async_provider(FfprobeCommandMediaStreamClassifierProvider)
        .expect_err("canonical selectors must have one owner");
    assert!(error.contains("ffprobe"));

    // The original alias still resolves after the rejected registration.
    let manager = OperationManager::new(Default::default()).unwrap();
    let group = manager.group(GroupOptions { capacity: 2 }).unwrap();
    let services = MimeExecutionServices::new(manager, group, Arc::new(FixedMimeExecutionPort::new(1, 2)), 32).unwrap();
    let (id, _backend) = registry
        .create_async_backend(
            &ProviderSelection::named("ffprobe-command").unwrap(),
            &MimeConfig::default(),
            &services,
        )
        .unwrap();
    assert_eq!("ffprobe", id);
}

#[test]
fn test_async_classifier_registry_seal_rejects_registration_through_clone_and_keeps_existing_provider() {
    let registry = MediaStreamClassifierRegistry::builtin();
    let clone = registry.clone();
    registry.seal();
    clone.seal();

    let error = clone
        .register_async_provider(FfprobeCommandMediaStreamClassifierProvider)
        .expect_err("sealing must reject later async registration, even through a clone");
    assert!(error.contains("sealed"), "unexpected rejection: {error}");

    let manager = OperationManager::new(Default::default()).unwrap();
    let group = manager.group(GroupOptions { capacity: 2 }).unwrap();
    let services = MimeExecutionServices::new(manager, group, Arc::new(FixedMimeExecutionPort::new(1, 2)), 32).unwrap();
    for selector in ["ffprobe", "ffprobe-command"] {
        let result = registry.create_async_backend(
            &ProviderSelection::named(selector).unwrap(),
            &MimeConfig::default(),
            &services,
        );
        assert!(
            result.is_ok(),
            "sealed registry must retain async selector `{selector}`"
        );
    }
}

#[derive(Debug)]
struct CountedAsyncClassifierProvider {
    descriptor: ProviderDescriptor,
    creations: Arc<AtomicUsize>,
}

impl ProviderMetadata for CountedAsyncClassifierProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        self.descriptor.clone()
    }
}

impl AsyncMediaStreamClassifierProvider for CountedAsyncClassifierProvider {
    fn create_async_backend(
        &self,
        _config: &MimeConfig,
        _services: &MimeExecutionServices,
    ) -> Result<Arc<dyn qubit_mime::AsyncMediaStreamClassifierBackend>, MimeError> {
        self.creations.fetch_add(1, Ordering::SeqCst);
        Err(MimeError::AsyncUnsupported)
    }
}

#[test]
fn test_async_classifier_absence_fallback_tries_next_registered_factory() {
    let registry = MediaStreamClassifierRegistry::default();
    let first_creations = Arc::new(AtomicUsize::new(0));
    let second_creations = Arc::new(AtomicUsize::new(0));
    for (id, creations) in [
        ("classifier-unsupported-first", Arc::clone(&first_creations)),
        ("classifier-unsupported-second", Arc::clone(&second_creations)),
    ] {
        registry
            .register_async_provider(CountedAsyncClassifierProvider {
                descriptor: ProviderDescriptor::new(ProviderId::new(id).unwrap()),
                creations,
            })
            .expect("async classifier should register");
    }

    let manager = OperationManager::new(Default::default()).unwrap();
    let group = manager.group(GroupOptions { capacity: 2 }).unwrap();
    let services = MimeExecutionServices::new(manager, group, Arc::new(FixedMimeExecutionPort::new(1, 2)), 32).unwrap();
    let selection = ProviderSelection::chain(["classifier-unsupported-first", "classifier-unsupported-second"])
        .unwrap()
        .with_fallback_policy(FallbackPolicy::OnAbsence);
    let result = registry.create_async_backend(&selection, &MimeConfig::default(), &services);

    assert!(matches!(result, Err(MimeError::AsyncUnsupported)));
    assert_eq!(1, first_creations.load(Ordering::SeqCst));
    assert_eq!(
        1,
        second_creations.load(Ordering::SeqCst),
        "AsyncUnsupported is absence and should fall through"
    );
}

#[derive(Debug)]
struct ReentrantClassifierProvider {
    registry: MediaStreamClassifierRegistry,
    descriptor: ProviderDescriptor,
    reentry_succeeded: Arc<AtomicBool>,
}

impl ProviderMetadata for ReentrantClassifierProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        self.descriptor.clone()
    }
}

impl AsyncMediaStreamClassifierProvider for ReentrantClassifierProvider {
    fn create_async_backend(
        &self,
        _config: &MimeConfig,
        _services: &MimeExecutionServices,
    ) -> Result<Arc<dyn qubit_mime::AsyncMediaStreamClassifierBackend>, MimeError> {
        let registry = self.registry.clone();
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let succeeded = registry
                .register_async_provider(FfprobeCommandMediaStreamClassifierProvider)
                .is_ok();
            let _ = sender.send(succeeded);
        });
        self.reentry_succeeded.store(
            receiver.recv_timeout(Duration::from_secs(1)).unwrap_or(false),
            std::sync::atomic::Ordering::SeqCst,
        );
        Err(MimeError::AsyncUnsupported)
    }
}

#[test]
fn test_async_classifier_factory_runs_without_holding_catalog_lock() {
    let registry = MediaStreamClassifierRegistry::default();
    let reentry_succeeded = Arc::new(AtomicBool::new(false));
    registry
        .register_async_provider(ReentrantClassifierProvider {
            registry: registry.clone(),
            descriptor: ProviderDescriptor::new(ProviderId::new("reentrant-classifier").unwrap()),
            reentry_succeeded: Arc::clone(&reentry_succeeded),
        })
        .unwrap();

    let manager = OperationManager::new(Default::default()).unwrap();
    let group = manager.group(GroupOptions { capacity: 2 }).unwrap();
    let services = MimeExecutionServices::new(manager, group, Arc::new(FixedMimeExecutionPort::new(1, 2)), 32).unwrap();
    assert!(
        registry
            .create_async_backend(
                &ProviderSelection::named("reentrant-classifier").unwrap(),
                &MimeConfig::default(),
                &services,
            )
            .is_err()
    );
    assert!(reentry_succeeded.load(std::sync::atomic::Ordering::SeqCst));
}
