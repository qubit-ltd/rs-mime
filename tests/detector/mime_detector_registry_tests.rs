// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::process::Command;
use std::ptr;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::time::Duration;

use qubit_mime::AsyncMimeDetectorProvider;
use qubit_mime::ContentRequirement;
use qubit_mime::FileCommandMimeDetectorProvider;
use qubit_mime::FixedMimeExecutionPort;
use qubit_mime::MimeConfig;
use qubit_mime::MimeDetector;
use qubit_mime::MimeDetectorRegistry;
use qubit_mime::MimeDetectorSpec;
use qubit_mime::MimeError;
use qubit_mime::MimeExecutionServices;
use qubit_mime::RepositoryMimeDetectorProvider;
use qubit_spi::FallbackPolicy;
use qubit_spi::ProviderCreationTermination;
use qubit_spi::ProviderDefinition;
use qubit_spi::ProviderDescriptor;
use qubit_spi::ProviderId;
use qubit_spi::ProviderMetadata;
use qubit_spi::ProviderSelection;
use qubit_spi::error::ProviderFailureKind;

use crate::support::TestMimeDetectorProvider;
use crate::support::TestProviderBehavior;

#[test]
fn test_builtin_registry_lists_and_resolves_repository_provider() {
    let registry = MimeDetectorRegistry::builtin();
    let expected_default = ProviderSelection::named("repository").expect("repository provider ID should be valid");
    let selection = ProviderSelection::named("repository-mime-detector").expect("repository alias should be valid");
    let detector = registry
        .resolve_selected(&selection)
        .expect("repository alias should resolve")
        .create()
        .expect("repository alias should resolve");
    assert_eq!(
        vec!["repository", "file"],
        registry.provider_ids().iter().map(|id| id.as_str()).collect::<Vec<_>>(),
    );
    assert_eq!(expected_default, registry.default_selection());
    assert_eq!(
        Some("application/pdf".to_owned()),
        detector
            .detect_by_filename("document.pdf")
            .expect("filename detection should succeed"),
    );
}

#[test]
fn test_global_registry_exposes_builtin_defaults_in_this_process() {
    let registry = MimeDetectorRegistry::global();

    assert!(registry.provider_ids().iter().any(|id| id.as_str() == "repository"),);
    assert_eq!(
        ProviderSelection::named("repository").expect("repository provider ID should be valid"),
        registry.default_selection(),
    );
}

#[test]
fn test_registry_seal_prevents_further_mutation() {
    let registry = MimeDetectorRegistry::builtin();
    assert!(!registry.is_sealed());

    registry.seal();

    assert!(registry.is_sealed());
    assert!(
        registry
            .register(TestMimeDetectorProvider::new(
                "after-seal",
                0,
                TestProviderBehavior::Success("application/x-after-seal"),
            ))
            .is_err()
    );
}

#[test]
fn test_registry_registers_owned_and_shared_providers_atomically() {
    let registry = MimeDetectorRegistry::default();
    registry
        .register(TestMimeDetectorProvider::new(
            "owned",
            20,
            TestProviderBehavior::Success("application/x-owned"),
        ))
        .expect("owned provider should register");
    let shared: Arc<dyn ProviderDefinition<MimeDetectorSpec>> = Arc::new(TestMimeDetectorProvider::new(
        "shared",
        10,
        TestProviderBehavior::Success("application/x-shared"),
    ));
    registry
        .register_shared(shared)
        .expect("shared provider should register");

    let duplicate = registry
        .register(TestMimeDetectorProvider::new(
            "shared",
            0,
            TestProviderBehavior::Success("application/x-duplicate"),
        ))
        .expect_err("duplicate selector should be rejected");
    assert_eq!(Some("shared"), duplicate.selector());

    let runtime_shared: Arc<dyn ProviderDefinition<MimeDetectorSpec>> = Arc::new(TestMimeDetectorProvider::new(
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
    let registry = MimeDetectorRegistry::default();
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
fn test_resolve_and_create_keep_selection_and_creation_errors_separate() {
    let registry = MimeDetectorRegistry::default();
    registry
        .register(TestMimeDetectorProvider::new(
            "failed",
            0,
            TestProviderBehavior::InitializationFailed,
        ))
        .expect("failing provider should register");
    let selection = ProviderSelection::named("failed").expect("test selector should be valid");
    let provider = registry
        .resolve_selected(&selection)
        .expect("provider selection should succeed before creation");
    let error = provider
        .create()
        .expect_err("selected provider should fail during creation");

    let attempt = error.decisive_attempt();
    assert_eq!("failed", attempt.provider_id().as_str());
    assert_eq!(ProviderFailureKind::InitializationFailed, attempt.failure().kind());
}

#[test]
fn test_default_selection_is_independent_from_service_configuration() {
    let registry = MimeDetectorRegistry::default();
    registry
        .register(TestMimeDetectorProvider::new(
            "configured",
            0,
            TestProviderBehavior::Success("application/x-configured"),
        ))
        .expect("configured provider should register");
    let _ = registry
        .set_default_selection(ProviderSelection::named("configured").expect("configured selector should be valid"));

    let provider = registry
        .resolve()
        .expect("registry default should resolve without MIME config");
    let config = MimeConfig::default();
    let explicit = provider
        .create_configured(&config)
        .expect("explicit MIME config should create a detector");
    let defaulted = provider.create().expect("default MIME config should create a detector");

    assert_eq!(
        Some("application/x-configured".to_owned()),
        explicit
            .detect_by_filename("sample.static")
            .expect("filename detection should succeed"),
    );
    assert_eq!(
        Some("application/x-configured".to_owned()),
        defaulted
            .detect_by_filename("sample.static")
            .expect("filename detection should succeed"),
    );
}

#[test]
fn test_resolving_provider_applies_selection_fallback_policy() {
    let registry = MimeDetectorRegistry::default();
    registry
        .register(TestMimeDetectorProvider::new(
            "unavailable",
            20,
            TestProviderBehavior::Unavailable,
        ))
        .expect("unavailable provider should register");
    registry
        .register(TestMimeDetectorProvider::new(
            "success",
            10,
            TestProviderBehavior::Success("application/x-static"),
        ))
        .expect("successful provider should register");
    let selection = ProviderSelection::chain(["unavailable", "success"])
        .expect("test chain should be valid")
        .with_fallback_policy(FallbackPolicy::OnAbsence);
    let detector = registry
        .resolve_selected(&selection)
        .expect("both candidates should resolve")
        .create()
        .expect("absence fallback should reach the successful provider");

    assert_eq!(
        Some("application/x-static".to_owned()),
        detector
            .detect_by_filename("sample.static")
            .expect("filename detection should succeed"),
    );
}

#[test]
fn test_resolving_provider_reports_policy_stop_with_actual_attempts() {
    let registry = MimeDetectorRegistry::default();
    registry
        .register(TestMimeDetectorProvider::new(
            "terminal",
            10,
            TestProviderBehavior::InitializationFailed,
        ))
        .expect("terminal provider should register");
    registry
        .register(TestMimeDetectorProvider::new(
            "unreached",
            0,
            TestProviderBehavior::Success("application/x-unreached"),
        ))
        .expect("unreached provider should register");
    let selection = ProviderSelection::chain(["terminal", "unreached"])
        .expect("test chain should be valid")
        .with_fallback_policy(FallbackPolicy::OnAbsence);
    let error = registry
        .resolve_selected(&selection)
        .expect("both providers should resolve")
        .create()
        .expect_err("initialization failure should stop absence fallback");

    assert_eq!(ProviderCreationTermination::StoppedByPolicy, error.termination(),);
    assert_eq!(1, error.attempts().len());
    assert_eq!("terminal", error.attempts()[0].provider_id().as_str());
}

/// Simulates library X resolving and creating a default detector without
/// knowing the App-selected implementation.
///
/// # Returns
///
/// The detector created through the process-wide Registry defaults.
fn library_x_create_detector() -> Arc<dyn MimeDetector> {
    MimeDetectorRegistry::global()
        .resolve()
        .expect("App-configured global provider should resolve")
        .create()
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
        let selection = ProviderSelection::named(PROVIDER_ID).expect("App provider selector should be valid");

        let explicit = registry
            .resolve_selected(&selection)
            .expect("explicit App selection should resolve")
            .create_configured(&MimeConfig::default())
            .expect("explicit MIME config should create the App detector");
        let _ = registry.set_default_selection(selection);
        let defaulted = library_x_create_detector();

        assert_eq!(
            Some("application/x-app-global".to_owned()),
            explicit
                .detect_by_filename("sample.static")
                .expect("filename detection should succeed"),
        );
        assert_eq!(
            Some("application/x-app-global".to_owned()),
            defaulted
                .detect_by_filename("sample.static")
                .expect("filename detection should succeed"),
        );
        return;
    }

    let current_test = std::env::current_exe().expect("current integration test executable should be available");
    let test_name = "detector::mime_detector_registry_tests::test_global_registry_shares_app_provider_with_library_x";
    let status = Command::new(current_test)
        .args(["--exact", test_name, "--nocapture"])
        .env(CHILD_MARKER, "1")
        .status()
        .expect("isolated global Registry scenario should start");

    assert!(status.success(), "isolated global Registry scenario failed");
}

#[derive(Debug)]
struct CountedAsyncRegistryProvider {
    descriptor: ProviderDescriptor,
    creations: Arc<AtomicUsize>,
}

#[derive(Debug)]
struct FailingAsyncRegistryProvider {
    descriptor: ProviderDescriptor,
    creations: Arc<AtomicUsize>,
    initialization_failure: bool,
}

impl ProviderMetadata for FailingAsyncRegistryProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        self.descriptor.clone()
    }
}

impl AsyncMimeDetectorProvider for FailingAsyncRegistryProvider {
    fn create_async_backend(
        &self,
        _config: &MimeConfig,
        _services: &MimeExecutionServices,
    ) -> Result<Arc<dyn qubit_mime::AsyncMimeDetectorBackend>, MimeError> {
        self.creations.fetch_add(1, Ordering::SeqCst);
        if self.initialization_failure {
            Err(MimeError::DetectorBackend {
                backend: "test".to_owned(),
                reason: self.descriptor.id().as_str().to_owned(),
            })
        } else {
            Err(MimeError::AsyncUnsupported)
        }
    }
}

impl ProviderMetadata for CountedAsyncRegistryProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        self.descriptor.clone()
    }
}

impl qubit_mime::AsyncMimeDetectorProvider for CountedAsyncRegistryProvider {
    fn create_async_backend(
        &self,
        _config: &MimeConfig,
        _services: &qubit_mime::MimeExecutionServices,
    ) -> Result<Arc<dyn qubit_mime::AsyncMimeDetectorBackend>, qubit_mime::MimeError> {
        self.creations.fetch_add(1, Ordering::SeqCst);
        Err(qubit_mime::MimeError::AsyncUnsupported)
    }
}

#[test]
fn test_async_registry_aliases_are_shared_by_clones_and_failed_registration_is_atomic() {
    let registry = MimeDetectorRegistry::default();
    let creations = Arc::new(AtomicUsize::new(0));
    let provider = CountedAsyncRegistryProvider {
        descriptor: ProviderDescriptor::new(ProviderId::new("async-owner").unwrap())
            .with_aliases(["async-alias"])
            .unwrap(),
        creations: Arc::clone(&creations),
    };
    registry
        .register_async_provider(provider)
        .expect("async provider should register");
    assert_eq!(
        vec!["async-owner"],
        registry
            .async_provider_ids()
            .iter()
            .map(|id| id.as_str())
            .collect::<Vec<_>>(),
        "async ID listing should omit aliases and deduplicate canonical providers",
    );
    assert_eq!(
        0,
        creations.load(Ordering::SeqCst),
        "registration must not construct a backend"
    );

    let clone = registry.clone();
    let conflicting_provider = CountedAsyncRegistryProvider {
        descriptor: ProviderDescriptor::new(ProviderId::new("partial-registration").unwrap())
            .with_aliases(["async-alias"])
            .unwrap(),
        creations: Arc::new(AtomicUsize::new(0)),
    };
    assert!(clone.register_async_provider(conflicting_provider).is_err());
    assert_eq!(
        0,
        creations.load(Ordering::SeqCst),
        "rejected registration must not construct a backend"
    );

    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let group = manager.group(qubit_operation::GroupOptions { capacity: 1 }).unwrap();
    let services = qubit_mime::MimeExecutionServices::new(
        manager,
        group,
        Arc::new(qubit_mime::FixedMimeExecutionPort::new(1, 1)),
        1024,
    )
    .unwrap();
    let alias = ProviderSelection::named("async-alias").unwrap();
    assert!(matches!(
        clone.create_async_backend(&alias, &MimeConfig::default(), &services),
        Err(qubit_mime::MimeError::AsyncUnsupported),
    ));
    assert_eq!(
        1,
        creations.load(Ordering::SeqCst),
        "clone should resolve the registered alias"
    );

    let partial_id = ProviderSelection::named("partial-registration").unwrap();
    let error = registry
        .create_async_backend(&partial_id, &MimeConfig::default(), &services)
        .expect_err("failed async registration must not claim its canonical selector");
    assert!(error.to_string().contains("partial-registration"));
    assert_eq!(
        1,
        creations.load(Ordering::SeqCst),
        "failed alias collision must not install the attempted provider's canonical ID",
    );

    let duplicate_alias_chain = ProviderSelection::chain(["async-alias", "async-owner"])
        .unwrap()
        .with_fallback_policy(FallbackPolicy::OnAnyError);
    assert!(clone
        .create_async_backend(&duplicate_alias_chain, &MimeConfig::default(), &services)
        .is_err());
    assert_eq!(
        2,
        creations.load(Ordering::SeqCst),
        "a canonical selector and its alias must not invoke one provider twice",
    );
}

#[test]
fn test_async_detector_absence_fallback_tries_next_registered_factory() {
    let registry = MimeDetectorRegistry::default();
    let first_creations = Arc::new(AtomicUsize::new(0));
    let second_creations = Arc::new(AtomicUsize::new(0));
    for (id, creations) in [
        ("async-unsupported-first", Arc::clone(&first_creations)),
        ("async-unsupported-second", Arc::clone(&second_creations)),
    ] {
        registry
            .register_async_provider(CountedAsyncRegistryProvider {
                descriptor: ProviderDescriptor::new(ProviderId::new(id).unwrap()),
                creations,
            })
            .expect("async provider should register");
    }

    let selection = ProviderSelection::chain(["async-unsupported-first", "async-unsupported-second"])
        .unwrap()
        .with_fallback_policy(FallbackPolicy::OnAbsence);
    let result = registry.create_async_backend(&selection, &MimeConfig::default(), &services());

    assert!(matches!(result, Err(MimeError::AsyncUnsupported)));
    assert_eq!(1, first_creations.load(Ordering::SeqCst));
    assert_eq!(1, second_creations.load(Ordering::SeqCst), "AsyncUnsupported is absence and should fall through");
}

#[test]
fn test_async_detector_fallback_policies_classify_factory_errors() {
    let services = services();
    let config = MimeConfig::default();

    let never_registry = MimeDetectorRegistry::default();
    let never_first = Arc::new(AtomicUsize::new(0));
    let never_second = Arc::new(AtomicUsize::new(0));
    for (id, creations) in [
        ("never-first", Arc::clone(&never_first)),
        ("never-second", Arc::clone(&never_second)),
    ] {
        never_registry
            .register_async_provider(FailingAsyncRegistryProvider {
                descriptor: ProviderDescriptor::new(ProviderId::new(id).unwrap()),
                creations,
                initialization_failure: false,
            })
            .unwrap();
    }
    let never = ProviderSelection::chain(["never-first", "never-second"])
        .unwrap()
        .with_fallback_policy(FallbackPolicy::Never);
    assert!(matches!(
        never_registry.create_async_backend(&never, &config, &services),
        Err(MimeError::AsyncUnsupported)
    ));
    assert_eq!(1, never_first.load(Ordering::SeqCst));
    assert_eq!(0, never_second.load(Ordering::SeqCst));

    let absence_registry = MimeDetectorRegistry::default();
    let absence_first = Arc::new(AtomicUsize::new(0));
    let absence_second = Arc::new(AtomicUsize::new(0));
    for (id, creations) in [
        ("init-failure-first", Arc::clone(&absence_first)),
        ("init-failure-second", Arc::clone(&absence_second)),
    ] {
        absence_registry
            .register_async_provider(FailingAsyncRegistryProvider {
                descriptor: ProviderDescriptor::new(ProviderId::new(id).unwrap()),
                creations,
                initialization_failure: true,
            })
            .unwrap();
    }
    let on_absence = ProviderSelection::chain(["init-failure-first", "init-failure-second"])
        .unwrap()
        .with_fallback_policy(FallbackPolicy::OnAbsence);
    let error = absence_registry
        .create_async_backend(&on_absence, &config, &services)
        .expect_err("initialization errors are not absence");
    assert!(error.to_string().contains("init-failure-first"));
    assert_eq!(1, absence_first.load(Ordering::SeqCst));
    assert_eq!(0, absence_second.load(Ordering::SeqCst));

    let any_error = ProviderSelection::chain(["init-failure-first", "init-failure-second"])
        .unwrap()
        .with_fallback_policy(FallbackPolicy::OnAnyError);
    let error = absence_registry
        .create_async_backend(&any_error, &config, &services)
        .expect_err("OnAnyError should continue after initialization failure");
    assert!(error.to_string().contains("init-failure-second"));
    assert_eq!(2, absence_first.load(Ordering::SeqCst));
    assert_eq!(1, absence_second.load(Ordering::SeqCst));
}

#[test]
fn test_async_detector_auto_uses_priority_and_stable_builtin_preference() {
    let registry = MimeDetectorRegistry::default();
    let low_priority = Arc::new(AtomicUsize::new(0));
    let high_priority = Arc::new(AtomicUsize::new(0));
    registry
        .register_async_provider(FailingAsyncRegistryProvider {
            descriptor: ProviderDescriptor::new(ProviderId::new("auto-low").unwrap()).with_priority(-1),
            creations: Arc::clone(&low_priority),
            initialization_failure: true,
        })
        .unwrap();
    registry
        .register_async_provider(FailingAsyncRegistryProvider {
            descriptor: ProviderDescriptor::new(ProviderId::new("auto-high").unwrap()).with_priority(100),
            creations: Arc::clone(&high_priority),
            initialization_failure: true,
        })
        .unwrap();

    let error = registry
        .create_async_backend(&ProviderSelection::auto(), &MimeConfig::default(), &services())
        .expect_err("the highest priority initialization failure should stop Auto fallback");
    assert!(error.to_string().contains("auto-high"));
    assert_eq!(1, high_priority.load(Ordering::SeqCst));
    assert_eq!(0, low_priority.load(Ordering::SeqCst));
}

#[test]
fn test_async_registry_seal_rejects_registration_through_clone_and_keeps_existing_provider() {
    let registry = MimeDetectorRegistry::builtin();
    let clone = registry.clone();
    registry.seal();
    clone.seal();

    let error = clone
        .register_async_provider(FileCommandMimeDetectorProvider)
        .expect_err("sealing must reject later async registration, even through a clone");
    assert!(error.contains("sealed"), "unexpected rejection: {error}");

    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let group = manager.group(qubit_operation::GroupOptions { capacity: 1 }).unwrap();
    let services = qubit_mime::MimeExecutionServices::new(
        manager,
        group,
        Arc::new(qubit_mime::FixedMimeExecutionPort::new(1, 1)),
        1024,
    )
    .unwrap();
    for selector in ["repository", "repository-mime-detector", "file", "file-command"] {
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
struct ReentrantDetectorProvider {
    registry: MimeDetectorRegistry,
    descriptor: ProviderDescriptor,
    reentry_succeeded: Arc<AtomicBool>,
}

impl ProviderMetadata for ReentrantDetectorProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        self.descriptor.clone()
    }
}

impl AsyncMimeDetectorProvider for ReentrantDetectorProvider {
    fn create_async_backend(
        &self,
        _config: &MimeConfig,
        _services: &MimeExecutionServices,
    ) -> Result<Arc<dyn qubit_mime::AsyncMimeDetectorBackend>, MimeError> {
        let registry = self.registry.clone();
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let succeeded = registry.register_async_provider(RepositoryMimeDetectorProvider).is_ok();
            let _ = sender.send(succeeded);
        });
        self.reentry_succeeded.store(
            receiver.recv_timeout(Duration::from_secs(1)).unwrap_or(false),
            Ordering::SeqCst,
        );
        Err(MimeError::AsyncUnsupported)
    }
}

#[test]
fn test_async_detector_factory_runs_without_holding_catalog_lock() {
    let registry = MimeDetectorRegistry::default();
    let reentry_succeeded = Arc::new(AtomicBool::new(false));
    registry
        .register_async_provider(ReentrantDetectorProvider {
            registry: registry.clone(),
            descriptor: ProviderDescriptor::new(ProviderId::new("reentrant-detector").unwrap()),
            reentry_succeeded: Arc::clone(&reentry_succeeded),
        })
        .unwrap();

    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let group = manager.group(qubit_operation::GroupOptions { capacity: 1 }).unwrap();
    let services = qubit_mime::MimeExecutionServices::new(
        manager,
        group,
        Arc::new(qubit_mime::FixedMimeExecutionPort::new(1, 1)),
        1024,
    )
    .unwrap();
    assert!(
        registry
            .create_async_backend(
                &ProviderSelection::named("reentrant-detector").unwrap(),
                &MimeConfig::default(),
                &services,
            )
            .is_err()
    );
    assert!(
        reentry_succeeded.load(Ordering::SeqCst),
        "factory callback could not register another provider"
    );
}

#[test]
fn test_registry_default_selection_follows_automatic_priority_and_seal() {
    let registry = MimeDetectorRegistry::default();
    registry
        .register(TestMimeDetectorProvider::new(
            "first",
            1,
            TestProviderBehavior::Success("application/x-first"),
        ))
        .expect("first provider should register");
    registry
        .register(TestMimeDetectorProvider::new(
            "preferred",
            20,
            TestProviderBehavior::Success("application/x-preferred"),
        ))
        .expect("preferred provider should register");

    assert_eq!(ProviderSelection::auto(), registry.default_selection());
    let selected = registry
        .resolve()
        .expect("automatic default should resolve registered providers")
        .create()
        .expect("highest-priority provider should create successfully");
    assert_eq!(
        Some("application/x-preferred".to_owned()),
        selected
            .detect_by_filename("sample.static")
            .expect("selected provider should handle the request"),
    );

    registry.seal();
    assert!(
        registry
            .set_default_selection(ProviderSelection::named("first").unwrap())
            .is_err()
    );
    assert_eq!(ProviderSelection::auto(), registry.default_selection());
}

fn services() -> MimeExecutionServices {
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let group = manager.group(qubit_operation::GroupOptions { capacity: 1 }).unwrap();
    MimeExecutionServices::new(manager, group, Arc::new(FixedMimeExecutionPort::new(1, 1)), 1024).unwrap()
}

#[test]
fn test_shared_provider_registration_drives_default_resolution() {
    let registry = MimeDetectorRegistry::default();
    let shared: Arc<dyn ProviderDefinition<MimeDetectorSpec>> = Arc::new(RepositoryMimeDetectorProvider);
    registry
        .register_shared(shared)
        .expect("shared provider should register in an empty registry");
    registry
        .set_default_selection(ProviderSelection::named("repository").unwrap())
        .expect("registered provider should be selectable as the default");

    let selection = registry.default_selection();
    assert_eq!(ProviderSelection::named("repository").unwrap(), selection);
    assert_eq!("repository", registry.provider_ids()[0].as_str());
    let detector = registry
        .resolve()
        .expect("stored default should resolve")
        .create()
        .expect("shared repository provider should create a detector");
    assert_eq!(
        Some("application/pdf".to_owned()),
        detector
            .detect_by_filename("document.pdf")
            .expect("resolved detector should serve filename requests")
    );

    let clone = registry.clone();
    assert_eq!(registry.provider_ids(), clone.provider_ids());
    assert_eq!(registry.default_selection(), clone.default_selection());
    registry.seal();
    assert!(registry.is_sealed());
}

#[test]
fn test_default_registry_starts_empty_and_auto_selection_is_unresolved() {
    let registry = MimeDetectorRegistry::default();

    assert!(registry.provider_ids().is_empty());
    assert!(registry.resolve().is_err());
    assert!(!registry.is_sealed());
    assert_eq!(ProviderSelection::auto(), registry.default_selection());

    registry
        .register(RepositoryMimeDetectorProvider)
        .expect("an empty default registry should accept its first provider");
    let detector = registry
        .resolve()
        .expect("auto selection should resolve after registration")
        .create()
        .expect("registered repository provider should construct");
    assert_eq!(
        Some("application/pdf".to_owned()),
        detector.detect_by_filename("document.pdf").unwrap()
    );
}

#[test]
fn test_global_registry_returns_process_stable_facade() {
    let first = MimeDetectorRegistry::global();
    let second = MimeDetectorRegistry::global();
    assert!(ptr::eq(first, second));
    assert!(first.provider_ids().iter().any(|id| id.as_str() == "repository"));
}

#[test]
fn test_builtin_async_providers_create_backends_without_spawning_commands() {
    let registry = MimeDetectorRegistry::builtin();
    let services = services();
    let config = MimeConfig::default();
    let external_creations = Arc::new(AtomicUsize::new(0));
    registry
        .register_async_provider(FailingAsyncRegistryProvider {
            descriptor: ProviderDescriptor::new(ProviderId::new("external-high-priority").unwrap())
                .with_priority(i32::MAX),
            creations: Arc::clone(&external_creations),
            initialization_failure: true,
        })
        .unwrap();

    let (auto_id, auto_backend) = registry
        .create_async_backend(&ProviderSelection::auto(), &config, &services)
        .expect("automatic async selection should keep the built-in repository ahead of externals");
    assert_eq!("repository", auto_id);
    assert_eq!(0, external_creations.load(Ordering::SeqCst));
    assert!(matches!(
        auto_backend.content_requirement(),
        ContentRequirement::Prefix(_)
    ));

    let (repository_id, repository_backend) = registry
        .create_async_backend(&ProviderSelection::named("repository").unwrap(), &config, &services)
        .expect("repository async provider should construct without external services");
    assert_eq!("repository", repository_id);
    assert!(matches!(
        repository_backend.content_requirement(),
        ContentRequirement::Prefix(_)
    ));

    let (file_id, file_backend) = registry
        .create_async_backend(&ProviderSelection::named("file").unwrap(), &config, &services)
        .expect("file async backend construction should not probe the command");
    assert_eq!("file", file_id);
    assert_eq!(ContentRequirement::Complete, file_backend.content_requirement());
}

#[test]
fn test_async_provider_strict_chain_reports_unknown_selector() {
    let registry = MimeDetectorRegistry::builtin();
    let selection = ProviderSelection::chain(["not-registered", "file"])
        .expect("chain selectors should be valid");

    assert!(registry
        .create_async_backend(&selection, &MimeConfig::default(), &services())
        .is_err(), "strict chains should diagnose missing selectors instead of skipping them");
}

#[test]
fn test_async_provider_chain_allowing_missing_uses_first_registered_backend() {
    let registry = MimeDetectorRegistry::builtin();
    let selection = ProviderSelection::chain_allowing_missing(["not-registered", "file", "repository"])
        .expect("chain selectors should be valid");

    let (id, backend) = registry
        .create_async_backend(&selection, &MimeConfig::default(), &services())
        .expect("lenient chain should continue to the first available async provider");

    assert_eq!("file", id);
    assert_eq!(ContentRequirement::Complete, backend.content_requirement());
}

#[test]
fn test_async_provider_chain_treats_known_sync_only_selector_as_absence() {
    let registry = MimeDetectorRegistry::builtin();
    registry
        .register(TestMimeDetectorProvider::new(
            "sync-only",
            100,
            TestProviderBehavior::Success("application/x-sync-only"),
        ))
        .unwrap();
    let selection = ProviderSelection::chain(["sync-only", "file"])
        .unwrap()
        .with_fallback_policy(FallbackPolicy::OnAbsence);

    let (id, backend) = registry
        .create_async_backend(&selection, &MimeConfig::default(), &services())
        .expect("known sync-only selector should allow fallback to the async file backend");

    assert_eq!("file", id);
    assert_eq!(ContentRequirement::Complete, backend.content_requirement());
}

#[test]
fn test_duplicate_shared_provider_registration_is_atomic() {
    let registry = MimeDetectorRegistry::default();
    let provider: Arc<dyn ProviderDefinition<MimeDetectorSpec>> = Arc::new(RepositoryMimeDetectorProvider);
    registry
        .register_shared(provider)
        .expect("first shared provider registration should succeed");
    let duplicate: Arc<dyn ProviderDefinition<MimeDetectorSpec>> = Arc::new(RepositoryMimeDetectorProvider);

    assert!(registry.register_shared(duplicate).is_err());
    assert_eq!(1, registry.provider_ids().len());
    assert_eq!("repository", registry.provider_ids()[0].as_str());
}

#[test]
fn test_registry_selection_errors_and_seal_preserve_catalog_state() {
    let registry = MimeDetectorRegistry::builtin();
    let missing = ProviderSelection::named("missing-provider").unwrap();
    registry
        .set_default_selection(missing.clone())
        .expect("selection mutation is separate from provider resolution");
    assert_eq!(missing, registry.default_selection());
    assert!(registry.resolve().is_err());
    assert!(registry.resolve_selected(&missing).is_err());
    registry
        .set_default_selection(ProviderSelection::named("repository").unwrap())
        .unwrap();
    registry.seal();
    assert!(registry.is_sealed());
    assert!(
        registry
            .set_default_selection(ProviderSelection::named("file").unwrap())
            .is_err()
    );
    assert!(registry.register(RepositoryMimeDetectorProvider).is_err());
    assert_eq!(2, registry.provider_ids().len());
}

#[test]
fn test_duplicate_async_provider_selector_is_rejected_without_replacing_builtin() {
    let registry = MimeDetectorRegistry::builtin();
    assert!(
        registry
            .register_async_provider(RepositoryMimeDetectorProvider)
            .is_err()
    );
    let (id, backend) = registry
        .create_async_backend(
            &ProviderSelection::named("repository").unwrap(),
            &MimeConfig::default(),
            &services(),
        )
        .unwrap();
    assert_eq!("repository", id);
    assert!(matches!(backend.content_requirement(), ContentRequirement::Prefix(_)));
}

#[test]
fn test_async_resolution_of_unknown_selector_reports_diagnostic() {
    let registry = MimeDetectorRegistry::builtin();
    let services = services();

    let error = registry.create_async_backend(
            &ProviderSelection::named("not-registered").unwrap(),
            &MimeConfig::default(),
            &services
        ).expect_err("unknown selector should have a selection diagnostic");
    assert!(error.to_string().contains("not-registered"));
}
