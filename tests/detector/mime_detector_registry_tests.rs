/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/

use std::sync::{
    Arc,
    atomic::{
        AtomicUsize,
        Ordering,
    },
};

use tempfile::TempDir;

use qubit_mime::{
    FileCommandMimeDetectorProvider,
    MimeConfig,
    MimeDetectionPolicy,
    MimeDetector,
    MimeDetectorAvailability,
    MimeDetectorRegistry,
    MimeDetectorSpec,
    MimeError,
    MimeResult,
    ProviderCreateError,
    ProviderDescriptor,
    ProviderRegistryError,
    ServiceProvider,
};

#[derive(Debug)]
struct NamedDetector {
    mime_type: &'static str,
}

impl MimeDetector for NamedDetector {
    fn detect_by_filename(&self, _filename: &str) -> Option<String> {
        Some(self.mime_type.to_owned())
    }

    fn detect_by_content(&self, _content: &[u8]) -> Option<String> {
        Some(self.mime_type.to_owned())
    }

    fn detect(
        &self,
        _content: &[u8],
        _filename: Option<&str>,
        _policy: MimeDetectionPolicy,
    ) -> Option<String> {
        Some(self.mime_type.to_owned())
    }

    fn detect_reader(
        &self,
        _reader: &mut dyn qubit_io::ReadSeek,
        _filename: Option<&str>,
        _policy: MimeDetectionPolicy,
    ) -> MimeResult<Option<String>> {
        Ok(Some(self.mime_type.to_owned()))
    }

    fn detect_file(
        &self,
        _file: &std::path::Path,
        _policy: MimeDetectionPolicy,
    ) -> MimeResult<Option<String>> {
        Ok(Some(self.mime_type.to_owned()))
    }
}

#[derive(Debug)]
struct TestProvider {
    id: &'static str,
    aliases: &'static [&'static str],
    mime_type: &'static str,
    priority: i32,
    unavailable: bool,
    created: AtomicUsize,
}

impl TestProvider {
    /// Creates a provider used by registry tests.
    fn new(id: &'static str, aliases: &'static [&'static str], mime_type: &'static str) -> Self {
        Self {
            id,
            aliases,
            mime_type,
            priority: 0,
            unavailable: false,
            created: AtomicUsize::new(0),
        }
    }

    /// Sets the provider priority.
    fn with_priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }

    /// Marks the provider as unavailable.
    fn unavailable(mut self) -> Self {
        self.unavailable = true;
        self
    }
}

impl ServiceProvider<MimeDetectorSpec> for TestProvider {
    fn descriptor(&self) -> Result<ProviderDescriptor, ProviderRegistryError> {
        Ok(ProviderDescriptor::new(self.id)?
            .with_aliases(self.aliases)?
            .with_priority(self.priority))
    }

    fn availability(&self, _config: &MimeConfig) -> MimeDetectorAvailability {
        if self.unavailable {
            MimeDetectorAvailability::unavailable("disabled for test")
        } else {
            MimeDetectorAvailability::Available
        }
    }

    fn create_box(
        &self,
        _config: &MimeConfig,
    ) -> Result<Box<dyn MimeDetector>, ProviderCreateError> {
        self.created.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(NamedDetector {
            mime_type: self.mime_type,
        }))
    }
}

#[derive(Debug)]
struct DefaultMethodProvider;

impl ServiceProvider<MimeDetectorSpec> for DefaultMethodProvider {
    fn descriptor(&self) -> Result<ProviderDescriptor, ProviderRegistryError> {
        ProviderDescriptor::new("default-method")
    }

    fn create_box(
        &self,
        _config: &MimeConfig,
    ) -> Result<Box<dyn MimeDetector>, ProviderCreateError> {
        Ok(Box::new(NamedDetector {
            mime_type: "application/x-default-method",
        }))
    }
}

#[derive(Debug)]
struct FailingProvider;

impl ServiceProvider<MimeDetectorSpec> for FailingProvider {
    fn descriptor(&self) -> Result<ProviderDescriptor, ProviderRegistryError> {
        ProviderDescriptor::new("failing")
    }

    fn create_box(
        &self,
        _config: &MimeConfig,
    ) -> Result<Box<dyn MimeDetector>, ProviderCreateError> {
        Err(ProviderCreateError::failed("forced failure"))
    }
}

#[test]
fn test_registry_creates_detector_by_id_and_alias_case_insensitively() {
    let mut registry = MimeDetectorRegistry::new();
    registry
        .register(TestProvider::new(
            "custom",
            &["custom-detector", "custom-mime-detector"],
            "application/x-custom",
        ))
        .expect("custom provider should register");

    let config = MimeConfig::default();
    let by_id = registry
        .create_box("custom", &config)
        .expect("provider id should resolve");
    let by_alias = registry
        .create_box("CUSTOM-MIME-DETECTOR", &config)
        .expect("provider alias should resolve");

    assert_eq!(
        Some("application/x-custom".to_owned()),
        by_id.detect_by_filename("file.bin")
    );
    assert_eq!(
        Some("application/x-custom".to_owned()),
        by_alias.detect_by_content(b"data")
    );
}

#[test]
fn test_registry_rejects_duplicate_provider_names_and_aliases() {
    let mut registry = MimeDetectorRegistry::new();
    registry
        .register(TestProvider::new(
            "custom",
            &["custom-detector"],
            "application/x-custom",
        ))
        .expect("first provider should register");

    let error = registry
        .register(TestProvider::new(
            "other",
            &["CUSTOM-DETECTOR"],
            "application/x-other",
        ))
        .expect_err("duplicate alias should be rejected");

    assert!(matches!(
        error,
        MimeError::DuplicateDetectorName { ref name } if name == "custom-detector"
    ));
}

#[test]
fn test_registry_registers_multiple_shared_providers() {
    let mut registry = MimeDetectorRegistry::new();
    registry
        .register_shared(Arc::new(TestProvider::new(
            "shared",
            &["shared-detector"],
            "application/x-shared",
        )))
        .expect("shared provider should register");
    registry
        .register_shared(Arc::new(TestProvider::new(
            "arc",
            &["arc-detector"],
            "application/x-arc",
        )))
        .expect("second shared provider should register");

    let config = MimeConfig::default();
    assert_eq!(
        Some("application/x-shared".to_owned()),
        registry
            .create_box("shared-detector", &config)
            .expect("shared provider should create detector")
            .detect_by_filename("file.bin")
    );
    assert_eq!(
        Some("application/x-arc".to_owned()),
        registry
            .create_arc("arc-detector", &config)
            .expect("arc provider should create detector")
            .detect_by_content(b"data")
    );
}

#[test]
fn test_registry_reports_invalid_direct_selectors_and_provider_failures() {
    let mut registry = MimeDetectorRegistry::new();
    registry
        .register(TestProvider::new("disabled", &[], "application/x-disabled").unavailable())
        .expect("disabled provider should register");
    registry
        .register(FailingProvider)
        .expect("failing provider should register");
    let config = MimeConfig::default();

    assert!(matches!(
        registry
            .create_box("", &config)
            .expect_err("empty selector should be rejected"),
        MimeError::EmptyDetectorName
    ));
    assert!(matches!(
        registry
            .create_box("bad name", &config)
            .expect_err("invalid selector should be rejected"),
        MimeError::InvalidDetectorName { ref name, .. } if name == "bad name"
    ));
    assert!(matches!(
        registry
            .create_box("disabled", &config)
            .expect_err("unavailable provider should not create detector"),
        MimeError::DetectorUnavailable { ref name, ref reason }
            if name == "disabled" && reason == "disabled for test"
    ));
    assert!(matches!(
        registry
            .create_box("failing", &config)
            .expect_err("failing provider should not create detector"),
        MimeError::DetectorBackend { ref backend, ref reason }
            if backend == "failing" && reason == "forced failure"
    ));
}

#[test]
fn test_registry_auto_selects_highest_priority_available_provider() {
    let mut registry = MimeDetectorRegistry::new();
    registry
        .register(TestProvider::new("low", &[], "application/x-low").with_priority(1))
        .expect("low provider should register");
    registry
        .register(TestProvider::new("high", &[], "application/x-high").with_priority(10))
        .expect("high provider should register");

    let config = create_detector_config("auto", &[]);
    let detector = registry
        .create_default_box(&config)
        .expect("auto should create highest priority provider");

    assert_eq!(
        Some("application/x-high".to_owned()),
        detector.detect_by_filename("file.bin")
    );
}

#[test]
fn test_registry_auto_tie_breaks_by_provider_id() {
    let mut registry = MimeDetectorRegistry::new();
    registry
        .register(TestProvider::new("z-provider", &[], "application/x-z").with_priority(10))
        .expect("z provider should register");
    registry
        .register(TestProvider::new("a-provider", &[], "application/x-a").with_priority(10))
        .expect("a provider should register");

    let config = create_detector_config("auto", &[]);
    let detector = registry
        .create_default_box(&config)
        .expect("auto should create lexicographically first tied provider");

    assert_eq!(
        Some("application/x-a".to_owned()),
        detector.detect_by_filename("file.bin")
    );
}

#[test]
fn test_empty_registry_reports_no_available_detector() {
    let registry = MimeDetectorRegistry::new();
    let error = registry
        .create_default_box(&create_detector_config("auto", &[]))
        .expect_err("empty registry should not create detector");

    assert!(matches!(
        error,
        MimeError::NoAvailableDetector { ref reason } if reason == "detector registry is empty"
    ));
}

#[test]
fn test_registry_uses_fallback_chain_when_primary_is_unavailable() {
    let mut registry = MimeDetectorRegistry::new();
    registry
        .register(TestProvider::new("primary", &[], "application/x-primary").unavailable())
        .expect("primary provider should register");
    registry
        .register(TestProvider::new("fallback", &[], "application/x-fallback"))
        .expect("fallback provider should register");

    let config = create_detector_config("primary", &["fallback"]);
    let detector = registry
        .create_default_box(&config)
        .expect("fallback provider should be used");

    assert_eq!(
        Some("application/x-fallback".to_owned()),
        detector.detect_by_filename("file.bin")
    );
}

#[test]
fn test_provider_default_methods_return_available_zero_priority_without_aliases() {
    let provider = DefaultMethodProvider;
    let availability = provider.availability(&MimeConfig::default());
    let descriptor = provider
        .descriptor()
        .expect("default-method descriptor should be valid");

    assert_eq!("default-method", descriptor.id().as_str());
    assert!(descriptor.aliases().is_empty());
    assert_eq!(0, descriptor.priority());
    assert!(availability.is_available());
    assert_eq!(
        Some("application/x-default-method".to_owned()),
        provider
            .create_box(&MimeConfig::default())
            .expect("default provider should create detector")
            .detect_by_filename("file.bin")
    );
}

#[test]
fn test_builtin_registry_exposes_repository_and_file_command_providers() {
    let registry = MimeDetectorRegistry::builtin();
    let names = registry.provider_names();

    assert!(names.contains(&"repository"));
    assert!(names.contains(&"file"));
    assert!(registry.find_provider("repository-mime-detector").is_some());
    assert!(
        registry
            .find_provider("file-command-mime-detector")
            .is_some()
    );

    let detector = registry
        .create_box("repository", &MimeConfig::default())
        .expect("repository provider should create detector");

    assert_eq!(
        Some("application/pdf".to_owned()),
        detector.detect_by_filename("document.pdf")
    );

    let file_provider = registry
        .find_provider("file")
        .expect("file provider should be registered");
    assert_eq!(
        10,
        file_provider
            .descriptor()
            .expect("file provider descriptor should be valid")
            .priority()
    );
}

#[test]
fn test_resolve_provider_matches_find_provider_for_builtin_names() {
    let registry = MimeDetectorRegistry::builtin();
    for name in [
        "repository",
        "repository-mime-detector",
        "file",
        "file-command-mime-detector",
    ] {
        let via_resolve = registry
            .resolve_provider(name)
            .unwrap_or_else(|_| panic!("resolve_provider should succeed for '{name}'"));
        let via_find = registry
            .find_provider(name)
            .unwrap_or_else(|| panic!("find_provider should succeed for '{name}'"));
        assert!(
            std::ptr::eq(via_resolve, via_find),
            "find_provider should delegate to resolve_provider for '{name}'"
        );
    }
}

#[test]
fn test_resolve_provider_reports_empty_invalid_and_unknown_names() {
    let registry = MimeDetectorRegistry::builtin();

    assert!(matches!(
        registry
            .resolve_provider("")
            .expect_err("empty name should be rejected"),
        MimeError::EmptyDetectorName
    ));
    assert!(matches!(
        registry
            .resolve_provider("bad name")
            .expect_err("invalid name should be rejected"),
        MimeError::InvalidDetectorName { ref name, .. } if name == "bad name"
    ));
    assert!(matches!(
        registry
            .resolve_provider("missing-detector-provider")
            .expect_err("unknown name should be rejected"),
        MimeError::UnknownDetector { ref name } if name == "missing-detector-provider"
    ));
}

#[test]
fn test_default_registry_starts_with_builtin_providers() {
    let registry = MimeDetectorRegistry::default_registry()
        .expect("default registry snapshot should be available");
    let names = registry.provider_names();

    assert!(names.contains(&"repository"));
    assert!(names.contains(&"file"));
}

#[test]
fn test_register_default_provider_makes_default_registry_see_provider() {
    MimeDetectorRegistry::register_default(TestProvider::new(
        "global-test",
        &["global-test-detector"],
        "application/x-global-test",
    ))
    .expect("global test provider should register");

    let registry = MimeDetectorRegistry::default_registry()
        .expect("default registry snapshot should be available");
    let by_name = registry
        .create_box("global-test-detector", &MimeConfig::default())
        .expect("default registry should create provider by name");
    let config = create_detector_config("global-test", &[]);
    let by_config = registry
        .create_default_box(&config)
        .expect("default registry should use configured default");

    assert_eq!(
        Some("application/x-global-test".to_owned()),
        by_name.detect_by_filename("file.bin")
    );
    assert_eq!(
        Some("application/x-global-test".to_owned()),
        by_config.detect_by_content(b"data")
    );
}

#[test]
fn test_file_command_provider_reports_unavailable_without_path() {
    const CHILD_ENV: &str = "QUBIT_MIME_CHECK_FILE_PROVIDER_UNAVAILABLE";
    const TEST_NAME: &str = "detector::mime_detector_registry_tests::test_file_command_provider_reports_unavailable_without_path";

    if std::env::var_os(CHILD_ENV).is_some() {
        let provider = FileCommandMimeDetectorProvider;
        let availability = provider.availability(&MimeConfig::default());

        assert!(!availability.is_available());
        assert!(matches!(
            availability,
            MimeDetectorAvailability::Unavailable { ref reason }
                if reason == "`file` command is not available"
        ));
        return;
    }

    let temp_dir = TempDir::new().expect("empty PATH directory should be created");
    let output = std::process::Command::new(
        std::env::current_exe().expect("current test binary path should be available"),
    )
    .arg(TEST_NAME)
    .arg("--exact")
    .arg("--nocapture")
    .arg("--test-threads=1")
    .env(CHILD_ENV, "1")
    .env("PATH", temp_dir.path())
    .output()
    .expect("child test process should run");

    assert!(
        output.status.success(),
        "child test failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn create_detector_config(default: &str, fallbacks: &[&str]) -> MimeConfig {
    let mut config = qubit_config::Config::new();
    config
        .set(qubit_mime::CONFIG_MIME_DETECTOR_DEFAULT, default)
        .expect("detector default should be configurable");
    config
        .set(
            qubit_mime::CONFIG_MIME_DETECTOR_FALLBACKS,
            fallbacks.join(","),
        )
        .expect("detector fallbacks should be configurable");
    MimeConfig::from_config(&config).expect("detector config should parse")
}
