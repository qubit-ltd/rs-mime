/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/

use std::io::Read;
use std::sync::Arc;

use qubit_mime::{
    FfprobeCommandMediaStreamClassifierProvider,
    MediaStreamClassifier,
    MediaStreamClassifierAvailability,
    MediaStreamClassifierRegistry,
    MediaStreamClassifierSpec,
    MediaStreamType,
    MimeConfig,
    MimeError,
    MimeResult,
    ProviderCreateError,
    ProviderDescriptor,
    ProviderRegistryError,
    ServiceProvider,
};

#[derive(Debug)]
struct NamedClassifier {
    stream_type: MediaStreamType,
}

impl MediaStreamClassifier for NamedClassifier {
    fn classify_file(&self, _file: &std::path::Path) -> MimeResult<MediaStreamType> {
        Ok(self.stream_type)
    }

    fn classify_reader(&self, _reader: &mut dyn Read) -> MimeResult<MediaStreamType> {
        Ok(self.stream_type)
    }
}

#[derive(Debug)]
struct TestProvider {
    id: &'static str,
    aliases: &'static [&'static str],
    stream_type: MediaStreamType,
    priority: i32,
    unavailable: bool,
}

impl TestProvider {
    /// Creates a classifier provider used by registry tests.
    fn new(
        id: &'static str,
        aliases: &'static [&'static str],
        stream_type: MediaStreamType,
    ) -> Self {
        Self {
            id,
            aliases,
            stream_type,
            priority: 0,
            unavailable: false,
        }
    }

    /// Sets the provider priority.
    fn with_priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }

    /// Marks the provider unavailable.
    fn unavailable(mut self) -> Self {
        self.unavailable = true;
        self
    }
}

impl ServiceProvider<MediaStreamClassifierSpec> for TestProvider {
    fn descriptor(&self) -> Result<ProviderDescriptor, ProviderRegistryError> {
        Ok(ProviderDescriptor::new(self.id)?
            .with_aliases(self.aliases)?
            .with_priority(self.priority))
    }

    fn availability(&self, _config: &MimeConfig) -> MediaStreamClassifierAvailability {
        if self.unavailable {
            MediaStreamClassifierAvailability::unavailable("disabled for test")
        } else {
            MediaStreamClassifierAvailability::Available
        }
    }

    fn create_box(
        &self,
        _config: &MimeConfig,
    ) -> Result<Box<dyn MediaStreamClassifier>, ProviderCreateError> {
        Ok(Box::new(NamedClassifier {
            stream_type: self.stream_type,
        }))
    }
}

#[derive(Debug)]
struct DefaultMethodProvider;

impl ServiceProvider<MediaStreamClassifierSpec> for DefaultMethodProvider {
    fn descriptor(&self) -> Result<ProviderDescriptor, ProviderRegistryError> {
        ProviderDescriptor::new("default-method")
    }

    fn create_box(
        &self,
        _config: &MimeConfig,
    ) -> Result<Box<dyn MediaStreamClassifier>, ProviderCreateError> {
        Ok(Box::new(NamedClassifier {
            stream_type: MediaStreamType::AudioOnly,
        }))
    }
}

#[derive(Debug)]
struct FailingProvider;

impl ServiceProvider<MediaStreamClassifierSpec> for FailingProvider {
    fn descriptor(&self) -> Result<ProviderDescriptor, ProviderRegistryError> {
        ProviderDescriptor::new("failing")
    }

    fn create_box(
        &self,
        _config: &MimeConfig,
    ) -> Result<Box<dyn MediaStreamClassifier>, ProviderCreateError> {
        Err(ProviderCreateError::failed("forced failure"))
    }
}

#[test]
fn test_registry_creates_classifier_by_id_and_alias_case_insensitively() {
    let mut registry = MediaStreamClassifierRegistry::new();
    registry
        .register(TestProvider::new(
            "custom",
            &["custom-classifier"],
            MediaStreamType::AudioOnly,
        ))
        .expect("custom classifier provider should register");

    let config = MimeConfig::default();
    let by_id = registry
        .create_box("custom", &config)
        .expect("provider id should resolve");
    let by_alias = registry
        .create_arc("CUSTOM-CLASSIFIER", &config)
        .expect("provider alias should resolve");

    assert_eq!(
        MediaStreamType::AudioOnly,
        by_id
            .classify_content(b"media")
            .expect("boxed classifier should classify content")
    );
    assert_eq!(
        MediaStreamType::AudioOnly,
        by_alias
            .classify_content(b"media")
            .expect("shared classifier should classify content")
    );
}

#[test]
fn test_registry_rejects_duplicate_provider_names_and_aliases() {
    let mut registry = MediaStreamClassifierRegistry::new();
    registry
        .register(TestProvider::new(
            "custom",
            &["custom-classifier"],
            MediaStreamType::AudioOnly,
        ))
        .expect("first provider should register");

    let error = registry
        .register(TestProvider::new(
            "other",
            &["CUSTOM-CLASSIFIER"],
            MediaStreamType::VideoOnly,
        ))
        .expect_err("duplicate alias should be rejected");

    assert!(matches!(
        error,
        MimeError::DuplicateClassifierName { ref name } if name == "custom-classifier"
    ));
}

#[test]
fn test_registry_registers_shared_and_arc_providers() {
    let mut registry = MediaStreamClassifierRegistry::new();
    registry
        .register_shared(Arc::new(TestProvider::new(
            "shared",
            &["shared-classifier"],
            MediaStreamType::AudioOnly,
        )))
        .expect("shared provider should register");
    registry
        .register_arc(Arc::new(TestProvider::new(
            "arc",
            &["arc-classifier"],
            MediaStreamType::VideoOnly,
        )))
        .expect("arc provider should register");

    let config = MimeConfig::default();
    assert_eq!(
        MediaStreamType::AudioOnly,
        registry
            .create_box("shared-classifier", &config)
            .expect("shared provider should create classifier")
            .classify_content(b"media")
            .expect("shared classifier should classify content")
    );
    assert_eq!(
        MediaStreamType::VideoOnly,
        registry
            .create_arc("arc-classifier", &config)
            .expect("arc provider should create classifier")
            .classify_content(b"media")
            .expect("arc classifier should classify content")
    );
}

#[test]
fn test_registry_reports_invalid_direct_selectors_and_provider_failures() {
    let mut registry = MediaStreamClassifierRegistry::new();
    registry
        .register(TestProvider::new("disabled", &[], MediaStreamType::None).unavailable())
        .expect("disabled provider should register");
    registry
        .register(FailingProvider)
        .expect("failing provider should register");
    let config = MimeConfig::default();

    assert!(matches!(
        registry
            .create_box("", &config)
            .expect_err("empty selector should be rejected"),
        MimeError::EmptyClassifierName
    ));
    assert!(matches!(
        registry
            .create_box("bad name", &config)
            .expect_err("invalid selector should be rejected"),
        MimeError::InvalidClassifierName { ref name, .. } if name == "bad name"
    ));
    assert!(matches!(
        registry
            .create_box("disabled", &config)
            .expect_err("unavailable provider should not create classifier"),
        MimeError::ClassifierUnavailable { ref name, ref reason }
            if name == "disabled" && reason == "disabled for test"
    ));
    assert!(matches!(
        registry
            .create_box("failing", &config)
            .expect_err("failing provider should not create classifier"),
        MimeError::ClassifierBackend { ref backend, ref reason }
            if backend == "failing" && reason == "forced failure"
    ));
}

#[test]
fn test_registry_auto_selects_highest_priority_available_provider() {
    let mut registry = MediaStreamClassifierRegistry::new();
    registry
        .register(TestProvider::new("low", &[], MediaStreamType::AudioOnly).with_priority(1))
        .expect("low provider should register");
    registry
        .register(TestProvider::new("high", &[], MediaStreamType::VideoOnly).with_priority(10))
        .expect("high provider should register");

    let classifier = registry
        .create_default_box(&create_classifier_config("auto"))
        .expect("auto should create highest priority provider");

    assert_eq!(
        MediaStreamType::VideoOnly,
        classifier
            .classify_content(b"media")
            .expect("selected classifier should classify content")
    );
}

#[test]
fn test_registry_creates_configured_default_by_name() {
    let mut registry = MediaStreamClassifierRegistry::new();
    registry
        .register(TestProvider::new(
            "primary",
            &[],
            MediaStreamType::VideoWithAudio,
        ))
        .expect("primary provider should register");

    let classifier = registry
        .create_default_arc(&create_classifier_config("primary"))
        .expect("configured selector should create matching provider");

    assert_eq!(
        MediaStreamType::VideoWithAudio,
        classifier
            .classify_content(b"media")
            .expect("selected classifier should classify content")
    );
}

#[test]
fn test_registry_skips_unavailable_provider_in_auto_selection() {
    let mut registry = MediaStreamClassifierRegistry::new();
    registry
        .register(
            TestProvider::new("unavailable", &[], MediaStreamType::AudioOnly)
                .with_priority(10)
                .unavailable(),
        )
        .expect("unavailable provider should register");
    registry
        .register(TestProvider::new(
            "available",
            &[],
            MediaStreamType::VideoOnly,
        ))
        .expect("available provider should register");

    let classifier = registry
        .create_default_box(&create_classifier_config("auto"))
        .expect("auto should skip unavailable providers");

    assert_eq!(
        MediaStreamType::VideoOnly,
        classifier
            .classify_content(b"media")
            .expect("selected classifier should classify content")
    );
}

#[test]
fn test_registry_reports_no_available_classifier_when_auto_candidates_fail() {
    let mut registry = MediaStreamClassifierRegistry::new();
    registry
        .register(TestProvider::new("disabled", &[], MediaStreamType::None).unavailable())
        .expect("disabled provider should register");
    registry
        .register(FailingProvider)
        .expect("failing provider should register");

    let error = registry
        .create_default_box(&create_classifier_config("auto"))
        .expect_err("auto should report exhausted classifier candidates");

    assert!(matches!(
        error,
        MimeError::NoAvailableClassifier { ref reason }
            if reason.contains("disabled") && reason.contains("failing")
    ));
}

#[test]
fn test_empty_registry_reports_no_available_classifier() {
    let registry = MediaStreamClassifierRegistry::new();
    let error = registry
        .create_default_box(&create_classifier_config("auto"))
        .expect_err("empty registry should not create classifier");

    assert!(matches!(
        error,
        MimeError::NoAvailableClassifier { ref reason }
            if reason == "classifier registry is empty"
    ));
}

#[test]
fn test_registry_reports_unknown_classifier_by_name() {
    let registry = MediaStreamClassifierRegistry::builtin();
    let error = registry
        .create_box("missing", &MimeConfig::default())
        .expect_err("unknown classifier should not resolve");

    assert!(matches!(
        error,
        MimeError::UnknownClassifier { ref name } if name == "missing"
    ));
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
        MediaStreamType::AudioOnly,
        provider
            .create_box(&MimeConfig::default())
            .expect("default provider should create classifier")
            .classify_content(b"media")
            .expect("classifier should classify content")
    );
}

#[test]
fn test_builtin_registry_exposes_ffprobe_provider() {
    let registry = MediaStreamClassifierRegistry::builtin();
    let names = registry.provider_names();

    assert!(names.contains(&"ffprobe"));
    assert!(registry.find_provider("ffprobe-command").is_some());
    assert!(
        registry
            .find_provider("ffprobe-command-media-stream-classifier")
            .is_some()
    );

    let provider = registry
        .find_provider("ffprobe")
        .expect("ffprobe provider should be registered");
    assert_eq!(
        10,
        provider
            .descriptor()
            .expect("ffprobe provider descriptor should be valid")
            .priority()
    );
    assert!(matches!(
        provider.availability(&MimeConfig::default()),
        MediaStreamClassifierAvailability::Available
            | MediaStreamClassifierAvailability::Unavailable { .. }
    ));
}

#[test]
fn test_ffprobe_provider_metadata_matches_builtin_registry_entry() {
    let provider = FfprobeCommandMediaStreamClassifierProvider;
    let descriptor = provider
        .descriptor()
        .expect("ffprobe provider descriptor should be valid");

    assert_eq!("ffprobe", descriptor.id().as_str());
    assert_eq!(
        vec!["ffprobe-command", "ffprobe-command-media-stream-classifier"],
        descriptor.aliases_as_str()
    );
    assert_eq!(10, descriptor.priority());
}

#[test]
fn test_register_default_provider_makes_default_registry_see_provider() {
    MediaStreamClassifierRegistry::register_default(TestProvider::new(
        "global-test",
        &["global-test-classifier"],
        MediaStreamType::VideoWithAudio,
    ))
    .expect("global test provider should register");

    let registry = MediaStreamClassifierRegistry::default_registry()
        .expect("default registry snapshot should be available");
    let by_name = registry
        .create_box("global-test-classifier", &MimeConfig::default())
        .expect("default registry should create provider by name");
    let by_config = registry
        .create_default_arc(&create_classifier_config("global-test"))
        .expect("default registry should use configured default");

    assert_eq!(
        MediaStreamType::VideoWithAudio,
        by_name
            .classify_content(b"media")
            .expect("named default classifier should classify content")
    );
    assert_eq!(
        MediaStreamType::VideoWithAudio,
        by_config
            .classify_content(b"media")
            .expect("configured default classifier should classify content")
    );
}

/// Creates MIME config with one classifier selector.
fn create_classifier_config(classifier: &str) -> MimeConfig {
    let mut config = qubit_config::Config::new();
    config
        .set(
            qubit_mime::CONFIG_MEDIA_STREAM_CLASSIFIER_DEFAULT,
            classifier,
        )
        .expect("classifier default should be configurable");
    MimeConfig::from_config(&config).expect("classifier config should parse")
}
