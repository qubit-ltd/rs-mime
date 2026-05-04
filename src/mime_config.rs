/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Configuration values for MIME detection.
//!

use std::collections::{
    HashMap,
    HashSet,
};
use std::sync::{
    LazyLock,
    RwLock,
};

use qubit_config::{
    Config,
    options::{
        CollectionReadOptions,
        ConfigReadOptions,
        EmptyItemPolicy,
    },
};

use crate::{
    CONFIG_MEDIA_STREAM_CLASSIFIER_DEFAULT,
    CONFIG_MIME_AMBIGUOUS_MIME_MAPPING,
    CONFIG_MIME_DETECTOR_DEFAULT,
    CONFIG_MIME_ENABLE_PRECISE_DETECTION,
    CONFIG_MIME_PRECISE_DETECTION_PATTERNS,
    DEFAULT_ENABLE_PRECISE_DETECTION,
    DEFAULT_MEDIA_STREAM_CLASSIFIER,
    DEFAULT_MIME_DETECTOR,
    ENV_MEDIA_STREAM_CLASSIFIER_DEFAULT,
    ENV_MIME_DETECTOR_AMBIGUOUS_MIME_MAPPING,
    ENV_MIME_DETECTOR_DEFAULT,
    ENV_MIME_DETECTOR_ENABLE_PRECISE_DETECTION,
    ENV_MIME_DETECTOR_PRECISE_DETECTION_PATTERNS,
    MimeResult,
};

/// Runtime configuration for MIME detectors.
#[derive(Debug, Clone)]
pub struct MimeConfig {
    /// Default MIME detector selector.
    mime_detector_default: String,
    /// Default media stream classifier selector.
    media_stream_classifier_default: String,
    /// Whether precise media-stream detection is enabled.
    enable_precise_detection: bool,
    /// Extensions requiring precise detection.
    precise_detection_patterns: HashSet<String>,
    /// Ambiguous MIME mappings.
    ambiguous_mime_mapping: HashMap<String, [String; 2]>,
}

/// Default MIME configuration.
static DEFAULT_MIME_CONFIG: LazyLock<RwLock<MimeConfig>> =
    LazyLock::new(|| RwLock::new(MimeConfig::load()));

/// Value read options.
static VALUE_READ_OPTIONS: LazyLock<ConfigReadOptions> =
    LazyLock::new(ConfigReadOptions::env_friendly);

/// Mapping read options.
static MAPPING_READ_OPTIONS: LazyLock<ConfigReadOptions> = LazyLock::new(|| {
    ConfigReadOptions::env_friendly().with_collection_options(
        CollectionReadOptions::default()
            .with_split_scalar_strings(true)
            .with_delimiters([';'])
            .with_trim_items(true)
            .with_empty_item_policy(EmptyItemPolicy::Skip),
    )
});

/// Built-in precise detection patterns.
static DEFAULT_PRECISE_DETECTION_PATTERNS: &[&str] = &["webm", "ogg"];

/// Built-in ambiguous MIME mapping entries.
static DEFAULT_AMBIGUOUS_MIME_MAPPING_ENTRIES: &[&str] =
    &["webm:video/webm,audio/webm", "ogg:video/ogg,audio/ogg"];

impl MimeConfig {
    /// Loads configuration from environment variables and defaults.
    ///
    /// # Returns
    /// Configuration used by default detector instances.
    pub fn load() -> Self {
        match Self::from_env() {
            Ok(config) => config,
            Err(_) => Self::builtin_default(),
        }
    }

    /// Creates MIME configuration from a config object.
    ///
    /// # Parameters
    /// - `config`: Configuration object containing logical keys or environment
    ///   variable style keys.
    ///
    /// # Returns
    /// Parsed MIME configuration.
    ///
    /// # Errors
    /// Returns [`MimeError::Config`](crate::MimeError::Config) when a present
    /// configuration value cannot be converted to the expected type.
    pub fn from_config(config: &Config) -> MimeResult<Self> {
        let mime_detector_default = config.get_any_or_with(
            [CONFIG_MIME_DETECTOR_DEFAULT, ENV_MIME_DETECTOR_DEFAULT],
            DEFAULT_MIME_DETECTOR.to_owned(),
            &VALUE_READ_OPTIONS,
        )?;
        let media_stream_classifier_default = config.get_any_or_with(
            [
                CONFIG_MEDIA_STREAM_CLASSIFIER_DEFAULT,
                ENV_MEDIA_STREAM_CLASSIFIER_DEFAULT,
            ],
            DEFAULT_MEDIA_STREAM_CLASSIFIER.to_owned(),
            &VALUE_READ_OPTIONS,
        )?;
        let enable_precise_detection = config.get_any_or_with(
            [
                CONFIG_MIME_ENABLE_PRECISE_DETECTION,
                ENV_MIME_DETECTOR_ENABLE_PRECISE_DETECTION,
            ],
            DEFAULT_ENABLE_PRECISE_DETECTION,
            &VALUE_READ_OPTIONS,
        )?;
        let precise_detection_patterns = config.get_any_or_with(
            [
                CONFIG_MIME_PRECISE_DETECTION_PATTERNS,
                ENV_MIME_DETECTOR_PRECISE_DETECTION_PATTERNS,
            ],
            DEFAULT_PRECISE_DETECTION_PATTERNS,
            &VALUE_READ_OPTIONS,
        )?;
        let ambiguous_mime_mapping = config.get_any_or_with(
            [
                CONFIG_MIME_AMBIGUOUS_MIME_MAPPING,
                ENV_MIME_DETECTOR_AMBIGUOUS_MIME_MAPPING,
            ],
            DEFAULT_AMBIGUOUS_MIME_MAPPING_ENTRIES,
            &MAPPING_READ_OPTIONS,
        )?;
        Ok(Self {
            mime_detector_default,
            media_stream_classifier_default,
            enable_precise_detection,
            precise_detection_patterns: normalize_patterns(precise_detection_patterns),
            ambiguous_mime_mapping: build_ambiguous_mime_mapping(ambiguous_mime_mapping),
        })
    }

    /// Creates MIME configuration from process environment variables.
    ///
    /// # Returns
    /// Parsed MIME configuration.
    ///
    /// # Errors
    /// Returns [`MimeError::Config`](crate::MimeError::Config) when the
    /// environment cannot be represented by [`Config`].
    pub fn from_env() -> MimeResult<Self> {
        let config = Config::from_env()?;
        Self::from_config(&config)
    }

    /// Replaces the global default MIME configuration.
    ///
    /// # Parameters
    /// - `config`: Configuration to use for future default instances.
    pub fn set_default(config: Self) {
        let mut guard = DEFAULT_MIME_CONFIG
            .write()
            .expect("default MIME configuration lock should not be poisoned");
        *guard = config;
    }

    /// Reloads the global default MIME configuration from a config object.
    ///
    /// # Parameters
    /// - `config`: Configuration object used to build the new default.
    ///
    /// # Errors
    /// Returns [`MimeError::Config`](crate::MimeError::Config) when a present
    /// configuration value cannot be converted to the expected type.
    pub fn reload_default(config: &Config) -> MimeResult<()> {
        Self::set_default(Self::from_config(config)?);
        Ok(())
    }

    /// Reloads the global default MIME configuration from process environment.
    ///
    /// # Errors
    /// Returns [`MimeError::Config`](crate::MimeError::Config) when the
    /// environment cannot be represented by [`Config`].
    pub fn reload_default_from_env() -> MimeResult<()> {
        Self::set_default(Self::from_env()?);
        Ok(())
    }

    /// Gets the configured default MIME detector selector.
    ///
    /// # Returns
    /// Backend selector used by default detector wrappers.
    pub fn mime_detector_default(&self) -> &str {
        &self.mime_detector_default
    }

    /// Gets the configured default media stream classifier selector.
    ///
    /// # Returns
    /// Backend selector used by default classifier wrappers.
    pub fn media_stream_classifier_default(&self) -> &str {
        &self.media_stream_classifier_default
    }

    /// Tells whether precise media-stream detection is enabled.
    ///
    /// # Returns
    /// `true` when ambiguous media MIME types may be refined.
    pub fn enable_precise_detection(&self) -> bool {
        self.enable_precise_detection
    }

    /// Gets extensions requiring precise detection.
    ///
    /// # Returns
    /// Lowercase extension names without leading dots.
    pub fn precise_detection_patterns(&self) -> &HashSet<String> {
        &self.precise_detection_patterns
    }

    /// Gets ambiguous extension mappings.
    ///
    /// # Returns
    /// Mapping from extension to `[video_mime, audio_mime]`.
    pub fn ambiguous_mime_mapping(&self) -> &HashMap<String, [String; 2]> {
        &self.ambiguous_mime_mapping
    }

    /// Creates the built-in MIME configuration.
    ///
    /// # Returns
    /// Configuration populated entirely from crate constants.
    fn builtin_default() -> Self {
        Self {
            mime_detector_default: DEFAULT_MIME_DETECTOR.to_owned(),
            media_stream_classifier_default: DEFAULT_MEDIA_STREAM_CLASSIFIER.to_owned(),
            enable_precise_detection: DEFAULT_ENABLE_PRECISE_DETECTION,
            precise_detection_patterns: normalize_patterns(
                DEFAULT_PRECISE_DETECTION_PATTERNS
                    .iter()
                    .map(|pattern| pattern.to_string())
                    .collect(),
            ),
            ambiguous_mime_mapping: build_ambiguous_mime_mapping(
                DEFAULT_AMBIGUOUS_MIME_MAPPING_ENTRIES
                    .iter()
                    .map(|entry| entry.to_string())
                    .collect(),
            ),
        }
    }
}

impl Default for MimeConfig {
    /// Loads default configuration.
    fn default() -> Self {
        DEFAULT_MIME_CONFIG
            .read()
            .expect("default MIME configuration lock should not be poisoned")
            .clone()
    }
}

/// Normalizes extension patterns.
///
/// # Parameters
/// - `patterns`: Raw extension items, usually read from configuration.
///
/// # Returns
/// Lowercase extension set without leading dots.
fn normalize_patterns(patterns: Vec<String>) -> HashSet<String> {
    patterns
        .into_iter()
        .map(|pattern| pattern.trim().to_owned())
        .filter(|pattern| !pattern.is_empty())
        .map(|pattern| pattern.trim_start_matches('.').to_ascii_lowercase())
        .collect()
}

/// Builds ambiguous MIME mappings from configured entries.
///
/// # Parameters
/// - `entries`: Mapping entries in `ext:video,audio` format.
///
/// # Returns
/// Lowercase extension to MIME pair mapping.
fn build_ambiguous_mime_mapping(entries: Vec<String>) -> HashMap<String, [String; 2]> {
    entries
        .into_iter()
        .filter_map(|entry| {
            let (extension, mime_types) = entry.split_once(':')?;
            let mut mime_types = mime_types.split(',').map(str::trim);
            let video_type = mime_types.next()?.to_owned();
            let audio_type = mime_types.next()?.to_owned();
            if extension.trim().is_empty()
                || video_type.is_empty()
                || audio_type.is_empty()
                || mime_types.next().is_some()
            {
                None
            } else {
                Some((
                    extension
                        .trim()
                        .trim_start_matches('.')
                        .to_ascii_lowercase(),
                    [video_type, audio_type],
                ))
            }
        })
        .collect()
}
