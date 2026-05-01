/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Tests for top-level MIME configuration defaults.

use std::sync::{Mutex, MutexGuard};

use qubit_config::Config;
use qubit_mime::{
    BoxMediaStreamClassifier, BoxMimeDetector, CONFIG_MEDIA_STREAM_CLASSIFIER_DEFAULT,
    CONFIG_MIME_AMBIGUOUS_MIME_MAPPING, CONFIG_MIME_DETECTOR_DEFAULT,
    CONFIG_MIME_ENABLE_PRECISE_DETECTION, CONFIG_MIME_PRECISE_DETECTION_PATTERNS,
    DEFAULT_AMBIGUOUS_MIME_MAPPING, DEFAULT_ENABLE_PRECISE_DETECTION,
    DEFAULT_MEDIA_STREAM_CLASSIFIER, DEFAULT_MIME_DETECTOR, DEFAULT_PRECISE_DETECTION_PATTERNS,
    ENV_MEDIA_STREAM_CLASSIFIER_DEFAULT, ENV_MIME_DETECTOR_AMBIGUOUS_MIME_MAPPING,
    ENV_MIME_DETECTOR_DEFAULT, ENV_MIME_DETECTOR_ENABLE_PRECISE_DETECTION,
    ENV_MIME_DETECTOR_PRECISE_DETECTION_PATTERNS, MimeConfig, MimeDetector,
};

static MIME_CONFIG_TEST_LOCK: Mutex<()> = Mutex::new(());

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

    let mime_config = MimeConfig::from_config(&config).expect("config should parse");

    assert_eq!("repository", mime_config.mime_detector_default());
    assert_eq!("ffprobe", mime_config.media_stream_classifier_default());
    assert!(!mime_config.enable_precise_detection());
    assert!(mime_config.precise_detection_patterns().contains("mkv"));
    assert_eq!(
        Some(&["video/x-matroska".to_owned(), "audio/x-matroska".to_owned(),]),
        mime_config.ambiguous_mime_mapping().get("mkv")
    );
}

#[test]
fn test_set_default_and_reload_default_replace_default_snapshot() {
    let _guard = mime_config_test_lock();
    let original = MimeConfig::default();
    let _restore = DefaultConfigRestore::new(original);
    let custom = MimeConfig::new(
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
fn test_wrappers_use_mime_config_defaults() {
    let _guard = mime_config_test_lock();
    let original = MimeConfig::default();
    let _restore = DefaultConfigRestore::new(original);

    MimeConfig::set_default(MimeConfig::new(
        "repository",
        "ffprobe",
        DEFAULT_ENABLE_PRECISE_DETECTION,
        DEFAULT_PRECISE_DETECTION_PATTERNS,
        DEFAULT_AMBIGUOUS_MIME_MAPPING,
    ));

    let detector = BoxMimeDetector::default();
    let _classifier = BoxMediaStreamClassifier::default();

    assert_eq!(
        DEFAULT_MIME_DETECTOR,
        MimeConfig::default().mime_detector_default()
    );
    assert_eq!(
        DEFAULT_MEDIA_STREAM_CLASSIFIER,
        MimeConfig::default().media_stream_classifier_default()
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
