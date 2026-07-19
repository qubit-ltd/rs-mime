// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Tests for top-level MIME configuration defaults.

use std::sync::{
    Mutex,
    MutexGuard,
};

use qubit_config::Config;
use qubit_mime::{
    CONFIG_COMMAND_OUTPUT_MAX_BYTES,
    CONFIG_MEDIA_STREAM_CLASSIFIER_DEFAULT,
    CONFIG_MEDIA_STREAM_MAX_STAGING_SIZE,
    CONFIG_MIME_AMBIGUOUS_MIME_MAPPING,
    CONFIG_MIME_DETECTOR_DEFAULT,
    CONFIG_MIME_DETECTOR_FALLBACKS,
    CONFIG_MIME_ENABLE_PRECISE_DETECTION,
    CONFIG_MIME_MAX_BUFFER_SIZE,
    CONFIG_MIME_PRECISE_DETECTION_PATTERNS,
    DEFAULT_AMBIGUOUS_MIME_MAPPING,
    DEFAULT_COMMAND_OUTPUT_MAX_BYTES,
    DEFAULT_ENABLE_PRECISE_DETECTION,
    DEFAULT_MEDIA_STREAM_CLASSIFIER,
    DEFAULT_MEDIA_STREAM_MAX_STAGING_SIZE,
    DEFAULT_MIME_DETECTOR,
    DEFAULT_MIME_MAX_BUFFER_SIZE,
    DEFAULT_PRECISE_DETECTION_PATTERNS,
    ENV_COMMAND_OUTPUT_MAX_BYTES,
    ENV_MEDIA_STREAM_CLASSIFIER_DEFAULT,
    ENV_MEDIA_STREAM_MAX_STAGING_SIZE,
    ENV_MIME_DETECTOR_AMBIGUOUS_MIME_MAPPING,
    ENV_MIME_DETECTOR_DEFAULT,
    ENV_MIME_DETECTOR_ENABLE_PRECISE_DETECTION,
    ENV_MIME_DETECTOR_PRECISE_DETECTION_PATTERNS,
    ENV_MIME_MAX_BUFFER_SIZE,
    MimeConfig,
    MimeDetector,
    MimeDetectorRegistry,
    MimeError,
};
use qubit_spi::error::ProviderResolutionError;
use qubit_spi::{
    ProviderSelection,
    ProviderSelectionTargetRef,
};

static MIME_CONFIG_TEST_LOCK: Mutex<()> = Mutex::new(());

/// Verifies provider selections are validated and retained during loading.
#[test]
fn test_mime_config_retains_validated_provider_selections() {
    let mut config = Config::new();
    config
        .set(CONFIG_MIME_DETECTOR_DEFAULT, "file")
        .expect("detector default should be configurable");
    config
        .set(CONFIG_MIME_DETECTOR_FALLBACKS, "repository")
        .expect("detector fallbacks should be configurable");
    config
        .set(CONFIG_MEDIA_STREAM_CLASSIFIER_DEFAULT, "auto")
        .expect("classifier default should be configurable");

    let mime_config = MimeConfig::from_config(&config)
        .expect("valid selections should parse");

    assert!(matches!(
        mime_config.mime_detector_selection().target(),
        ProviderSelectionTargetRef::Chain { selectors, .. }
            if selectors
                .iter()
                .map(|selector| selector.as_str())
                .eq(["file", "repository"])
    ));
    assert!(matches!(
        mime_config.media_stream_classifier_selection().target(),
        ProviderSelectionTargetRef::Auto,
    ));
}

/// Verifies malformed provider selections fail while configuration is loaded.
#[test]
fn test_mime_config_rejects_invalid_provider_selections() {
    let mut detector = Config::new();
    detector
        .set(CONFIG_MIME_DETECTOR_DEFAULT, "bad selector")
        .expect("invalid detector default should be storable");
    assert!(matches!(
        MimeConfig::from_config(&detector),
        Err(MimeError::InvalidDetectorName { ref name, .. })
            if name == "bad selector"
    ));

    let mut classifier = Config::new();
    classifier
        .set(CONFIG_MEDIA_STREAM_CLASSIFIER_DEFAULT, "bad selector")
        .expect("invalid classifier default should be storable");
    assert!(matches!(
        MimeConfig::from_config(&classifier),
        Err(MimeError::InvalidClassifierName { ref name, .. })
            if name == "bad selector"
    ));
}

/// Verifies that configured fallback chains reject unregistered providers.
#[test]
fn test_mime_config_uses_strict_detector_chain_resolution() {
    let mut config = Config::new();
    config
        .set(CONFIG_MIME_DETECTOR_DEFAULT, "missing")
        .expect("detector default should be configurable");
    config
        .set(CONFIG_MIME_DETECTOR_FALLBACKS, "repository")
        .expect("detector fallbacks should be configurable");
    let mime_config = MimeConfig::from_config(&config)
        .expect("selector syntax should be valid");

    let error = MimeDetectorRegistry::builtin()
        .resolve_selected(mime_config.mime_detector_selection())
        .expect_err("strict chain should reject the missing provider");

    assert!(matches!(
        error,
        ProviderResolutionError::UnknownProviders { selectors, .. }
            if selectors.len() == 1 && selectors[0].as_str() == "missing"
    ));
}

#[test]
fn test_from_config_reads_logical_config_keys() {
    let mut config = Config::new();
    config
        .set(CONFIG_MIME_DETECTOR_DEFAULT, "repository")
        .expect("detector default should be configurable");
    config
        .set(CONFIG_MEDIA_STREAM_CLASSIFIER_DEFAULT, "ffprobe")
        .expect("classifier default should be configurable");
    config
        .set(CONFIG_MIME_ENABLE_PRECISE_DETECTION, "no")
        .expect("precise detection should be configurable");
    config
        .set(CONFIG_MIME_PRECISE_DETECTION_PATTERNS, ".mkv, webm")
        .expect("precise patterns should be configurable");
    config
        .set(
            CONFIG_MIME_AMBIGUOUS_MIME_MAPPING,
            "mkv:video/x-matroska,audio/x-matroska",
        )
        .expect("ambiguous mapping should be configurable");
    config
        .set(CONFIG_MIME_MAX_BUFFER_SIZE, 4096_u64)
        .expect("maximum buffer size should be configurable");
    config
        .set(CONFIG_MEDIA_STREAM_MAX_STAGING_SIZE, 8_388_608_u64)
        .expect("maximum staging size should be configurable");

    let mime_config =
        MimeConfig::from_config(&config).expect("config should parse");

    assert_eq!(
        "repository",
        selection_primary(mime_config.mime_detector_selection()),
    );
    assert_eq!(
        "ffprobe",
        selection_primary(mime_config.media_stream_classifier_selection()),
    );
    assert!(!mime_config.enable_precise_detection());
    assert!(mime_config.precise_detection_patterns().contains("mkv"));
    assert_eq!(
        Some(&["video/x-matroska".to_owned(), "audio/x-matroska".to_owned(),]),
        mime_config.ambiguous_mime_mapping().get("mkv")
    );
    assert_eq!(4096, mime_config.max_buffer_size());
    assert_eq!(8_388_608, mime_config.media_stream_max_staging_size());
}

#[test]
fn test_from_config_interpolates_provider_selectors() {
    let mut config = Config::new();
    config
        .set("preferred.detector", "repository")
        .expect("detector reference should be configurable");
    config
        .set(CONFIG_MIME_DETECTOR_DEFAULT, "${preferred.detector}")
        .expect("detector selector should be configurable");

    let mime_config = MimeConfig::from_config(&config)
        .expect("interpolated detector selector should parse");

    assert_eq!(
        "repository",
        selection_primary(mime_config.mime_detector_selection()),
    );
}

#[test]
fn test_from_config_reads_command_output_limit() {
    let mut config = Config::new();
    config
        .set(CONFIG_COMMAND_OUTPUT_MAX_BYTES, 1024_u64)
        .expect("command output limit should be configurable");

    let mime_config = MimeConfig::from_config(&config)
        .expect("command output limit should parse");

    assert_eq!(1024, mime_config.command_output_max_bytes());
}

#[test]
fn test_from_config_reads_env_aliases_with_env_friendly_options() {
    let mut config = Config::new();
    config
        .set(CONFIG_MIME_DETECTOR_DEFAULT, "   ")
        .expect("blank detector default should be configurable");
    config
        .set(ENV_MIME_DETECTOR_DEFAULT, "repository")
        .expect("detector env default should be configurable");
    config
        .set(ENV_MEDIA_STREAM_CLASSIFIER_DEFAULT, " ffprobe ")
        .expect("classifier env default should be configurable");
    config
        .set(ENV_MIME_DETECTOR_ENABLE_PRECISE_DETECTION, "yes")
        .expect("precise detection env flag should be configurable");
    config
        .set(
            ENV_MIME_DETECTOR_PRECISE_DETECTION_PATTERNS,
            ".mkv, webm,, ",
        )
        .expect("precise patterns env value should be configurable");
    config
        .set(
            ENV_MIME_DETECTOR_AMBIGUOUS_MIME_MAPPING,
            "mkv:video/x-matroska,audio/x-matroska; webm:video/webm,audio/webm",
        )
        .expect("ambiguous mapping env value should be configurable");
    config
        .set(ENV_MIME_MAX_BUFFER_SIZE, "8192")
        .expect("maximum buffer size env value should be configurable");
    config
        .set(ENV_MEDIA_STREAM_MAX_STAGING_SIZE, "16777216")
        .expect("maximum staging size env value should be configurable");
    config
        .set(ENV_COMMAND_OUTPUT_MAX_BYTES, "4096")
        .expect("command output limit env value should be configurable");

    let mime_config =
        MimeConfig::from_config(&config).expect("env aliases should parse");

    assert_eq!(
        "repository",
        selection_primary(mime_config.mime_detector_selection()),
    );
    assert_eq!(
        "ffprobe",
        selection_primary(mime_config.media_stream_classifier_selection()),
    );
    assert!(mime_config.enable_precise_detection());
    assert!(mime_config.precise_detection_patterns().contains("mkv"));
    assert!(mime_config.precise_detection_patterns().contains("webm"));
    assert_eq!(
        Some(&["video/webm".to_owned(), "audio/webm".to_owned()]),
        mime_config.ambiguous_mime_mapping().get("webm")
    );
    assert_eq!(8192, mime_config.max_buffer_size());
    assert_eq!(16_777_216, mime_config.media_stream_max_staging_size());
    assert_eq!(4096, mime_config.command_output_max_bytes());
}

#[test]
fn test_from_config_reports_invalid_boolean_value() {
    let mut config = Config::new();
    config
        .set(CONFIG_MIME_ENABLE_PRECISE_DETECTION, "maybe")
        .expect("invalid precise detection flag should still be storable");

    let result = MimeConfig::from_config(&config);

    assert!(matches!(result, Err(MimeError::Config(_))));
}

#[test]
fn test_reload_default_reports_invalid_config_and_environment() {
    let _guard = mime_config_test_lock();
    let _env_restore =
        EnvRestore::new(&[ENV_MIME_DETECTOR_ENABLE_PRECISE_DETECTION]);
    let mut config = Config::new();
    config
        .set(CONFIG_MIME_ENABLE_PRECISE_DETECTION, "maybe")
        .expect("invalid precise detection flag should still be storable");

    assert!(matches!(
        MimeConfig::reload_default(&config),
        Err(MimeError::Config(_))
    ));

    unsafe {
        std::env::set_var(ENV_MIME_DETECTOR_ENABLE_PRECISE_DETECTION, "maybe");
    }
    assert!(matches!(
        MimeConfig::reload_default_from_env(),
        Err(MimeError::Config(_))
    ));
    unsafe {
        std::env::remove_var(ENV_MIME_DETECTOR_ENABLE_PRECISE_DETECTION);
    }
}

#[test]
fn test_from_config_skips_blank_patterns_and_malformed_mapping_entries() {
    let mut config = Config::new();
    config
        .set(CONFIG_MIME_DETECTOR_DEFAULT, " ")
        .expect("blank detector default should be configurable");
    config
        .set(CONFIG_MEDIA_STREAM_CLASSIFIER_DEFAULT, "\t")
        .expect("blank classifier default should be configurable");
    config
        .set(CONFIG_MIME_ENABLE_PRECISE_DETECTION, false)
        .expect("precise detection flag should be configurable");
    config
        .set(CONFIG_MIME_PRECISE_DETECTION_PATTERNS, "webm,.ogg,, ")
        .expect("precise patterns should be configurable");
    config
        .set(
            CONFIG_MIME_AMBIGUOUS_MIME_MAPPING,
            "webm:video/webm,audio/webm;bad;empty:,audio/x;extra:video/x,audio/x,other",
        )
        .expect("ambiguous mapping should be configurable");

    let mime_config =
        MimeConfig::from_config(&config).expect("config should parse");

    assert_eq!(
        "repository",
        selection_primary(mime_config.mime_detector_selection()),
    );
    assert_eq!(
        "ffprobe",
        selection_primary(mime_config.media_stream_classifier_selection()),
    );
    assert!(!mime_config.enable_precise_detection());
    assert!(mime_config.precise_detection_patterns().contains("webm"));
    assert!(mime_config.precise_detection_patterns().contains("ogg"));
    assert_eq!(1, mime_config.ambiguous_mime_mapping().len());
    assert!(mime_config.ambiguous_mime_mapping().contains_key("webm"));
}

#[test]
fn test_load_falls_back_to_builtin_default_when_env_is_invalid() {
    let _guard = mime_config_test_lock();
    let _env_restore =
        EnvRestore::new(&[ENV_MIME_DETECTOR_ENABLE_PRECISE_DETECTION]);

    unsafe {
        std::env::set_var(ENV_MIME_DETECTOR_ENABLE_PRECISE_DETECTION, "maybe");
    }
    let loaded = MimeConfig::load();
    unsafe {
        std::env::remove_var(ENV_MIME_DETECTOR_ENABLE_PRECISE_DETECTION);
    }

    assert_eq!(
        DEFAULT_MIME_DETECTOR,
        selection_primary(loaded.mime_detector_selection()),
    );
    assert_eq!(
        DEFAULT_MEDIA_STREAM_CLASSIFIER,
        selection_primary(loaded.media_stream_classifier_selection())
    );
    assert_eq!(
        DEFAULT_ENABLE_PRECISE_DETECTION,
        loaded.enable_precise_detection()
    );
    assert_eq!(DEFAULT_MIME_MAX_BUFFER_SIZE, loaded.max_buffer_size());
    assert_eq!(
        DEFAULT_MEDIA_STREAM_MAX_STAGING_SIZE,
        loaded.media_stream_max_staging_size()
    );
    assert_eq!(
        DEFAULT_COMMAND_OUTPUT_MAX_BYTES,
        loaded.command_output_max_bytes()
    );
}

#[test]
fn test_load_uses_environment_when_valid() {
    let _guard = mime_config_test_lock();
    let _env_restore = EnvRestore::new(&[
        ENV_MIME_DETECTOR_DEFAULT,
        ENV_MEDIA_STREAM_CLASSIFIER_DEFAULT,
        ENV_MEDIA_STREAM_MAX_STAGING_SIZE,
        ENV_COMMAND_OUTPUT_MAX_BYTES,
        ENV_MIME_DETECTOR_ENABLE_PRECISE_DETECTION,
    ]);

    unsafe {
        std::env::set_var(ENV_MIME_DETECTOR_DEFAULT, "repository");
        std::env::set_var(ENV_MEDIA_STREAM_CLASSIFIER_DEFAULT, "ffprobe");
        std::env::set_var(ENV_MEDIA_STREAM_MAX_STAGING_SIZE, "33554432");
        std::env::set_var(ENV_COMMAND_OUTPUT_MAX_BYTES, "4096");
        std::env::set_var(ENV_MIME_DETECTOR_ENABLE_PRECISE_DETECTION, "false");
    }
    let loaded = MimeConfig::load();

    assert_eq!(
        "repository",
        selection_primary(loaded.mime_detector_selection()),
    );
    assert_eq!(
        "ffprobe",
        selection_primary(loaded.media_stream_classifier_selection()),
    );
    assert_eq!(33_554_432, loaded.media_stream_max_staging_size());
    assert_eq!(4096, loaded.command_output_max_bytes());
    assert!(!loaded.enable_precise_detection());
}

#[test]
fn test_set_default_and_reload_default_replace_default_snapshot() {
    let _guard = mime_config_test_lock();
    let original = MimeConfig::default();
    let _restore = DefaultConfigRestore::new(original);
    let custom = create_test_config(
        "repository",
        "ffprobe",
        true,
        "mkv,webm",
        "mkv:video/x-matroska,audio/x-matroska",
    );
    MimeConfig::set_default(custom);

    assert!(
        MimeConfig::default()
            .precise_detection_patterns()
            .contains("mkv")
    );

    let mut config = Config::new();
    config
        .set(CONFIG_MIME_DETECTOR_DEFAULT, "repository")
        .expect("detector default should be configurable");
    config
        .set(CONFIG_MEDIA_STREAM_CLASSIFIER_DEFAULT, "ffprobe")
        .expect("classifier default should be configurable");
    config
        .set(CONFIG_MIME_ENABLE_PRECISE_DETECTION, "true")
        .expect("precise detection should be configurable");
    config
        .set(CONFIG_MIME_PRECISE_DETECTION_PATTERNS, "avi")
        .expect("precise patterns should be configurable");
    config
        .set(
            CONFIG_MIME_AMBIGUOUS_MIME_MAPPING,
            "avi:video/x-msvideo,audio/x-msvideo",
        )
        .expect("ambiguous mapping should be configurable");

    MimeConfig::reload_default(&config).expect("default config should reload");

    assert!(
        MimeConfig::default()
            .precise_detection_patterns()
            .contains("avi")
    );
    assert!(
        !MimeConfig::default()
            .precise_detection_patterns()
            .contains("mkv")
    );
}

#[test]
fn test_reload_default_from_env_uses_config_from_env() {
    let _guard = mime_config_test_lock();
    let original = MimeConfig::default();
    let _restore = DefaultConfigRestore::new(original);
    let _env_restore = EnvRestore::new(&[
        ENV_MIME_DETECTOR_DEFAULT,
        ENV_MEDIA_STREAM_CLASSIFIER_DEFAULT,
        ENV_MIME_DETECTOR_ENABLE_PRECISE_DETECTION,
        ENV_MIME_DETECTOR_PRECISE_DETECTION_PATTERNS,
        ENV_MIME_DETECTOR_AMBIGUOUS_MIME_MAPPING,
    ]);

    unsafe {
        std::env::set_var(ENV_MIME_DETECTOR_DEFAULT, "repository");
        std::env::set_var(ENV_MEDIA_STREAM_CLASSIFIER_DEFAULT, "ffprobe");
        std::env::set_var(ENV_MIME_DETECTOR_ENABLE_PRECISE_DETECTION, "on");
        std::env::set_var(ENV_MIME_DETECTOR_PRECISE_DETECTION_PATTERNS, "avi");
        std::env::set_var(
            ENV_MIME_DETECTOR_AMBIGUOUS_MIME_MAPPING,
            "avi:video/x-msvideo,audio/x-msvideo",
        );
    }

    let result = MimeConfig::reload_default_from_env();

    unsafe {
        std::env::remove_var(ENV_MIME_DETECTOR_DEFAULT);
        std::env::remove_var(ENV_MEDIA_STREAM_CLASSIFIER_DEFAULT);
        std::env::remove_var(ENV_MIME_DETECTOR_ENABLE_PRECISE_DETECTION);
        std::env::remove_var(ENV_MIME_DETECTOR_PRECISE_DETECTION_PATTERNS);
        std::env::remove_var(ENV_MIME_DETECTOR_AMBIGUOUS_MIME_MAPPING);
    }

    result.expect("default config should reload from environment");
    assert!(
        MimeConfig::default()
            .precise_detection_patterns()
            .contains("avi")
    );
}

#[test]
fn test_registries_use_mime_config_defaults() {
    let _guard = mime_config_test_lock();
    let original = MimeConfig::default();
    let _restore = DefaultConfigRestore::new(original);

    MimeConfig::set_default(create_test_config(
        "repository",
        "ffprobe",
        DEFAULT_ENABLE_PRECISE_DETECTION,
        DEFAULT_PRECISE_DETECTION_PATTERNS,
        DEFAULT_AMBIGUOUS_MIME_MAPPING,
    ));

    let detector_registry = MimeDetectorRegistry::builtin();
    let detector = detector_registry
        .resolve()
        .expect("default detector selection")
        .create()
        .expect("default detector");

    assert_eq!(
        DEFAULT_MIME_DETECTOR,
        selection_primary(MimeConfig::default().mime_detector_selection())
    );
    assert_eq!(
        DEFAULT_MEDIA_STREAM_CLASSIFIER,
        selection_primary(
            MimeConfig::default().media_stream_classifier_selection(),
        )
    );
    assert_eq!(
        Some("application/pdf".to_owned()),
        detector.detect_by_filename("document.pdf")
    );
}

fn mime_config_test_lock() -> MutexGuard<'static, ()> {
    MIME_CONFIG_TEST_LOCK
        .lock()
        .expect("MIME config test lock should not be poisoned")
}

/// Returns the first explicit selector retained by a validated selection.
///
/// # Arguments
///
/// * `selection` - Named or chained selection under test.
///
/// # Returns
///
/// The named selector or first chain candidate.
///
/// # Panics
///
/// Panics when `selection` is automatic or contains no explicit selector.
fn selection_primary(selection: &ProviderSelection) -> &str {
    match selection.target() {
        ProviderSelectionTargetRef::Named(selector) => selector.as_str(),
        ProviderSelectionTargetRef::Chain { selectors, .. } => selectors
            .first()
            .expect("test chain should contain an explicit provider")
            .as_str(),
        ProviderSelectionTargetRef::Auto => {
            panic!("test selection should contain an explicit provider")
        }
    }
}

fn create_test_config(
    mime_detector_default: &str,
    media_stream_classifier_default: &str,
    enable_precise_detection: bool,
    precise_detection_patterns: &str,
    ambiguous_mime_mapping: &str,
) -> MimeConfig {
    let mut config = Config::new();
    config
        .set(CONFIG_MIME_DETECTOR_DEFAULT, mime_detector_default)
        .expect("detector default should be configurable");
    config
        .set(
            CONFIG_MEDIA_STREAM_CLASSIFIER_DEFAULT,
            media_stream_classifier_default,
        )
        .expect("classifier default should be configurable");
    config
        .set(
            CONFIG_MIME_ENABLE_PRECISE_DETECTION,
            enable_precise_detection,
        )
        .expect("precise detection should be configurable");
    config
        .set(
            CONFIG_MIME_PRECISE_DETECTION_PATTERNS,
            precise_detection_patterns,
        )
        .expect("precise detection patterns should be configurable");
    config
        .set(CONFIG_MIME_AMBIGUOUS_MIME_MAPPING, ambiguous_mime_mapping)
        .expect("ambiguous MIME mapping should be configurable");
    MimeConfig::from_config(&config).expect("test MIME config should parse")
}

struct DefaultConfigRestore {
    config: MimeConfig,
}

impl DefaultConfigRestore {
    fn new(config: MimeConfig) -> Self {
        Self { config }
    }
}

impl Drop for DefaultConfigRestore {
    fn drop(&mut self) {
        MimeConfig::set_default(self.config.clone());
    }
}

struct EnvRestore {
    values: Vec<(&'static str, Option<String>)>,
}

impl EnvRestore {
    fn new(keys: &[&'static str]) -> Self {
        Self {
            values: keys
                .iter()
                .map(|key| (*key, std::env::var(key).ok()))
                .collect(),
        }
    }
}

impl Drop for EnvRestore {
    fn drop(&mut self) {
        for (key, value) in &self.values {
            unsafe {
                if let Some(value) = value {
                    std::env::set_var(key, value);
                } else {
                    std::env::remove_var(key);
                }
            }
        }
    }
}
