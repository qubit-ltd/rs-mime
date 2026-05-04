/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/

use std::sync::atomic::{
    AtomicUsize,
    Ordering,
};

use tempfile::TempDir;

use qubit_mime::{
    FileCommandMimeDetectorProvider,
    MimeConfig,
    MimeDetectionPolicy,
    MimeDetector,
    MimeDetectorAvailability,
    MimeDetectorProvider,
    MimeDetectorRegistry,
    MimeError,
    MimeResult,
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

impl MimeDetectorProvider for TestProvider {
    fn id(&self) -> &'static str {
        self.id
    }

    fn aliases(&self) -> &'static [&'static str] {
        self.aliases
    }

    fn priority(&self) -> i32 {
        self.priority
    }

    fn availability(&self, _config: &MimeConfig) -> MimeDetectorAvailability {
        if self.unavailable {
            MimeDetectorAvailability::Unavailable {
                reason: "disabled for test".to_owned(),
            }
        } else {
            MimeDetectorAvailability::Available
        }
    }

    fn create(&self, _config: &MimeConfig) -> MimeResult<Box<dyn MimeDetector>> {
        self.created.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(NamedDetector {
            mime_type: self.mime_type,
        }))
    }
}

#[derive(Debug)]
struct DefaultMethodProvider;

impl MimeDetectorProvider for DefaultMethodProvider {
    fn id(&self) -> &'static str {
        "default-method"
    }

    fn create(&self, _config: &MimeConfig) -> MimeResult<Box<dyn MimeDetector>> {
        Ok(Box::new(NamedDetector {
            mime_type: "application/x-default-method",
        }))
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
        .create("custom", &config)
        .expect("provider id should resolve");
    let by_alias = registry
        .create("CUSTOM-MIME-DETECTOR", &config)
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
        MimeError::DuplicateDetectorName { ref name } if name == "CUSTOM-DETECTOR"
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
        .create_default(&config)
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
        .create_default(&config)
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
        .create_default(&create_detector_config("auto", &[]))
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
        .create_default(&config)
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

    assert_eq!("default-method", provider.id());
    assert_eq!(&[] as &[&str], provider.aliases());
    assert_eq!(0, provider.priority());
    assert!(availability.is_available());
    assert_eq!(
        Some("application/x-default-method".to_owned()),
        provider
            .create(&MimeConfig::default())
            .expect("default provider should create detector")
            .detect_by_filename("file.bin")
    );
}

#[test]
fn test_builtin_registry_exposes_repository_and_file_command_providers() {
    let registry = MimeDetectorRegistry::with_builtin();
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
        .create("repository", &MimeConfig::default())
        .expect("repository provider should create detector");

    assert_eq!(
        Some("application/pdf".to_owned()),
        detector.detect_by_filename("document.pdf")
    );

    let file_provider = registry
        .find_provider("file")
        .expect("file provider should be registered");
    assert_eq!(10, file_provider.priority());
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
