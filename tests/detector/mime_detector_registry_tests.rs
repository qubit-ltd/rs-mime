// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::Path;
use std::sync::Arc;

use qubit_config::Config;
use qubit_io::ReadSeek;
use qubit_mime::{
    CONFIG_MIME_DETECTOR_DEFAULT,
    CONFIG_MIME_DETECTOR_FALLBACKS,
    MimeConfig,
    MimeDetectionPolicy,
    MimeDetector,
    MimeDetectorRegistry,
    MimeDetectorSpec,
    MimeError,
    MimeResult,
};
use qubit_spi::error::ProviderError;
use qubit_spi::{
    ProviderDescriptor,
    ProviderId,
    ServiceProvider,
};

#[derive(Debug)]
struct StaticDetector;

impl MimeDetector for StaticDetector {
    fn detect_by_filename(&self, filename: &str) -> Option<String> {
        filename
            .ends_with(".static")
            .then(|| "application/x-static".to_owned())
    }

    fn detect_by_content(&self, _content: &[u8]) -> Option<String> {
        None
    }

    fn detect(
        &self,
        _content: &[u8],
        filename: Option<&str>,
        _policy: MimeDetectionPolicy,
    ) -> Option<String> {
        filename.and_then(|name| self.detect_by_filename(name))
    }

    fn detect_reader(
        &self,
        _reader: &mut dyn ReadSeek,
        filename: Option<&str>,
        policy: MimeDetectionPolicy,
    ) -> MimeResult<Option<String>> {
        Ok(self.detect(&[], filename, policy))
    }

    fn detect_file(
        &self,
        file: &Path,
        policy: MimeDetectionPolicy,
    ) -> MimeResult<Option<String>> {
        Ok(self.detect(
            &[],
            file.file_name().and_then(|name| name.to_str()),
            policy,
        ))
    }
}

#[derive(Clone, Copy, Debug)]
enum ProviderBehavior {
    Success,
    Unsupported,
    Unavailable,
    InitializationFailed,
}

#[derive(Debug)]
struct TestProvider(ProviderBehavior);

impl ServiceProvider<MimeDetectorSpec> for TestProvider {
    fn create(
        &self,
        _config: &MimeConfig,
    ) -> Result<Arc<dyn MimeDetector>, ProviderError> {
        match self.0 {
            ProviderBehavior::Success => Ok(Arc::new(StaticDetector)),
            ProviderBehavior::Unsupported => {
                Err(ProviderError::unsupported("unsupported input"))
            }
            ProviderBehavior::Unavailable => {
                Err(ProviderError::unavailable("missing executable"))
            }
            ProviderBehavior::InitializationFailed => {
                Err(ProviderError::initialization_failed("startup failed"))
            }
        }
    }
}

fn descriptor(id: &str, priority: i32) -> ProviderDescriptor {
    ProviderDescriptor::new(
        ProviderId::new(id).expect("test provider ID should be valid"),
    )
    .with_priority(priority)
}

fn detector_config(primary: &str, fallbacks: &[&str]) -> MimeConfig {
    let mut source = Config::new();
    source
        .set(CONFIG_MIME_DETECTOR_DEFAULT, primary)
        .expect("primary detector should be configurable");
    source
        .set(CONFIG_MIME_DETECTOR_FALLBACKS, fallbacks.join(","))
        .expect("detector fallbacks should be configurable");
    MimeConfig::from_config(&source).expect("detector config should parse")
}

#[test]
fn test_builtin_registry_lists_and_resolves_repository_provider() {
    let registry = MimeDetectorRegistry::builtin();
    let detector = registry
        .create("repository-mime-detector", &MimeConfig::default())
        .expect("repository alias should resolve");

    assert_eq!(vec!["repository", "file"], registry.provider_ids());
    assert_eq!(
        Some("application/pdf".to_owned()),
        detector.detect_by_filename("document.pdf"),
    );
}

#[test]
fn test_builder_registers_owned_and_shared_providers_atomically() {
    let mut builder = MimeDetectorRegistry::builder();
    builder
        .register(
            descriptor("owned", 20),
            TestProvider(ProviderBehavior::Success),
        )
        .expect("owned provider should register");
    builder
        .register_shared(
            descriptor("shared", 10),
            Arc::new(TestProvider(ProviderBehavior::Success)),
        )
        .expect("shared provider should register");

    let duplicate = builder
        .register(
            descriptor("shared", 0),
            TestProvider(ProviderBehavior::Success),
        )
        .expect_err("duplicate selector should be rejected");
    assert!(matches!(
        duplicate,
        MimeError::DuplicateDetectorName { ref name } if name == "shared"
    ));

    let registry = builder.build();
    assert_eq!(vec!["owned", "shared"], registry.provider_ids());
}

#[test]
fn test_create_maps_invalid_and_unknown_detector_selectors() {
    let registry = MimeDetectorRegistry::builder().build();
    let config = MimeConfig::default();

    assert!(matches!(
        registry.create("", &config),
        Err(MimeError::EmptyDetectorName)
    ));
    assert!(matches!(
        registry.create("bad selector", &config),
        Err(MimeError::InvalidDetectorName { ref name, ref reason })
            if name == "bad selector" && reason.contains("bad selector")
    ));
    assert!(matches!(
        registry.create("missing", &config),
        Err(MimeError::UnknownDetector { ref name }) if name == "missing"
    ));
    assert!(matches!(
        registry.create_default(&detector_config("auto", &[])),
        Err(MimeError::NoAvailableDetector { ref reason }) if !reason.is_empty()
    ));
}

/// Verifies that one configured unknown detector is reported as an unknown
/// selector when no fallback detector is configured.
#[test]
fn test_create_default_maps_single_unknown_detector() {
    let registry = MimeDetectorRegistry::builder().build();

    assert!(matches!(
        registry.create_default(&detector_config("missing", &[])),
        Err(MimeError::UnknownDetector { ref name }) if name == "missing"
    ));
}

#[test]
fn test_create_maps_single_provider_failures() {
    let mut builder = MimeDetectorRegistry::builder();
    builder
        .register(
            descriptor("unsupported", 0),
            TestProvider(ProviderBehavior::Unsupported),
        )
        .expect("unsupported provider should register");
    builder
        .register(
            descriptor("unavailable", 0),
            TestProvider(ProviderBehavior::Unavailable),
        )
        .expect("unavailable provider should register");
    builder
        .register(
            descriptor("failed", 0),
            TestProvider(ProviderBehavior::InitializationFailed),
        )
        .expect("failing provider should register");
    let registry = builder.build();
    let config = MimeConfig::default();

    for (selector, expected_reason) in [
        ("unsupported", "unsupported input"),
        ("unavailable", "missing executable"),
    ] {
        assert!(matches!(
            registry.create(selector, &config),
            Err(MimeError::DetectorUnavailable { ref name, ref reason })
                if name == selector && reason == expected_reason
        ));
    }
    assert!(matches!(
        registry.create("failed", &config),
        Err(MimeError::DetectorBackend { ref backend, ref reason })
            if backend == "failed" && reason == "startup failed"
    ));
}

#[test]
fn test_create_default_supports_auto_named_and_exhausted_chains() {
    let mut builder = MimeDetectorRegistry::builder();
    builder
        .register(
            descriptor("unavailable", 20),
            TestProvider(ProviderBehavior::Unavailable),
        )
        .expect("unavailable provider should register");
    builder
        .register(
            descriptor("success", 10),
            TestProvider(ProviderBehavior::Success),
        )
        .expect("successful provider should register");
    let registry = builder.build();

    let automatic = registry
        .create_default(&detector_config("AUTO", &[]))
        .expect("automatic resolution should reach the successful provider");
    let named = registry
        .create_default(&detector_config("success", &[]))
        .expect("configured resolution should use the named provider");
    assert_eq!(
        Some("application/x-static".to_owned()),
        automatic.detect_by_filename("sample.static")
    );
    assert_eq!(
        Some("application/x-static".to_owned()),
        named.detect_by_filename("sample.static")
    );
    assert!(matches!(
        registry.create_default(&detector_config("bad selector", &[])),
        Err(MimeError::InvalidDetectorName { .. })
    ));

    let mut exhausted = MimeDetectorRegistry::builder();
    exhausted
        .register(
            descriptor("first", 20),
            TestProvider(ProviderBehavior::Unavailable),
        )
        .expect("first provider should register");
    exhausted
        .register(
            descriptor("second", 10),
            TestProvider(ProviderBehavior::Unsupported),
        )
        .expect("second provider should register");
    assert!(matches!(
        exhausted
            .build()
            .create_default(&detector_config("first", &["second"])),
        Err(MimeError::NoAvailableDetector { ref reason })
            if reason.contains("missing executable")
                && reason.contains("unsupported input")
    ));
}
