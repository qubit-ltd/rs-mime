// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io::Read;
use std::path::Path;
use std::sync::Arc;

use qubit_config::Config;
use qubit_mime::{
    CONFIG_MEDIA_STREAM_CLASSIFIER_DEFAULT,
    MediaStreamClassifier,
    MediaStreamClassifierRegistry,
    MediaStreamClassifierSpec,
    MediaStreamType,
    MimeConfig,
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
struct StaticClassifier;

impl MediaStreamClassifier for StaticClassifier {
    fn classify_file(&self, _file: &Path) -> MimeResult<MediaStreamType> {
        Ok(MediaStreamType::None)
    }

    fn classify_reader(
        &self,
        _reader: &mut dyn Read,
    ) -> MimeResult<MediaStreamType> {
        Ok(MediaStreamType::None)
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

impl ServiceProvider<MediaStreamClassifierSpec> for TestProvider {
    fn create(
        &self,
        _config: &MimeConfig,
    ) -> Result<Arc<dyn MediaStreamClassifier>, ProviderError> {
        match self.0 {
            ProviderBehavior::Success => Ok(Arc::new(StaticClassifier)),
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

fn classifier_config(selector: &str) -> MimeConfig {
    let mut source = Config::new();
    source
        .set(CONFIG_MEDIA_STREAM_CLASSIFIER_DEFAULT, selector)
        .expect("classifier selector should be configurable");
    MimeConfig::from_config(&source).expect("classifier config should parse")
}

#[test]
fn test_builtin_registry_lists_and_resolves_ffprobe_provider() {
    let registry = MediaStreamClassifierRegistry::builtin();

    assert_eq!(vec!["ffprobe"], registry.provider_ids());
    registry
        .create("ffprobe-command", &MimeConfig::default())
        .expect("FFprobe alias should resolve");
}

#[test]
fn test_builder_registers_owned_and_shared_providers_atomically() {
    let mut builder = MediaStreamClassifierRegistry::builder();
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
        MimeError::DuplicateClassifierName { ref name } if name == "shared"
    ));

    let registry = builder.build();
    assert_eq!(vec!["owned", "shared"], registry.provider_ids());
}

#[test]
fn test_create_maps_invalid_and_unknown_classifier_selectors() {
    let registry = MediaStreamClassifierRegistry::builder().build();
    let config = MimeConfig::default();

    assert!(matches!(
        registry.create("", &config),
        Err(MimeError::EmptyClassifierName)
    ));
    assert!(matches!(
        registry.create("bad selector", &config),
        Err(MimeError::InvalidClassifierName { ref name, ref reason })
            if name == "bad selector" && reason.contains("bad selector")
    ));
    assert!(matches!(
        registry.create("missing", &config),
        Err(MimeError::UnknownClassifier { ref name }) if name == "missing"
    ));
    assert!(matches!(
        registry.create_default(&classifier_config("auto")),
        Err(MimeError::NoAvailableClassifier { ref reason }) if !reason.is_empty()
    ));
}

#[test]
fn test_create_maps_single_provider_failures() {
    let mut builder = MediaStreamClassifierRegistry::builder();
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
            Err(MimeError::ClassifierUnavailable { ref name, ref reason })
                if name == selector && reason == expected_reason
        ));
    }
    assert!(matches!(
        registry.create("failed", &config),
        Err(MimeError::ClassifierBackend { ref backend, ref reason })
            if backend == "failed" && reason == "startup failed"
    ));
}

#[test]
fn test_create_default_supports_auto_named_and_exhausted_selection() {
    let mut builder = MediaStreamClassifierRegistry::builder();
    builder
        .register(
            descriptor("unavailable", 30),
            TestProvider(ProviderBehavior::Unavailable),
        )
        .expect("unavailable provider should register");
    builder
        .register(
            descriptor("unsupported", 20),
            TestProvider(ProviderBehavior::Unsupported),
        )
        .expect("unsupported provider should register");
    builder
        .register(
            descriptor("success", 10),
            TestProvider(ProviderBehavior::Success),
        )
        .expect("successful provider should register");
    let registry = builder.build();

    registry
        .create_default(&classifier_config("AUTO"))
        .expect("automatic resolution should reach the successful provider");
    registry
        .create_default(&classifier_config("success"))
        .expect("configured resolution should use the named provider");
    assert!(matches!(
        registry.create_default(&classifier_config("bad selector")),
        Err(MimeError::InvalidClassifierName { .. })
    ));

    let mut exhausted = MediaStreamClassifierRegistry::builder();
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
            .create_default(&classifier_config("auto")),
        Err(MimeError::NoAvailableClassifier { ref reason })
            if reason.contains("missing executable")
                && reason.contains("unsupported input")
    ));
}
