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

use std::collections::{HashMap, HashSet};
use std::sync::{OnceLock, RwLock};

use qubit_config::Config;

use crate::{
    CONFIG_MEDIA_STREAM_CLASSIFIER_DEFAULT, CONFIG_MIME_AMBIGUOUS_MIME_MAPPING,
    CONFIG_MIME_DETECTOR_DEFAULT, CONFIG_MIME_ENABLE_PRECISE_DETECTION,
    CONFIG_MIME_PRECISE_DETECTION_PATTERNS, DEFAULT_AMBIGUOUS_MIME_MAPPING,
    DEFAULT_ENABLE_PRECISE_DETECTION, DEFAULT_MEDIA_STREAM_CLASSIFIER, DEFAULT_MIME_DETECTOR,
    DEFAULT_PRECISE_DETECTION_PATTERNS, ENV_MEDIA_STREAM_CLASSIFIER_DEFAULT,
    ENV_MIME_DETECTOR_AMBIGUOUS_MIME_MAPPING, ENV_MIME_DETECTOR_DEFAULT,
    ENV_MIME_DETECTOR_ENABLE_PRECISE_DETECTION, ENV_MIME_DETECTOR_PRECISE_DETECTION_PATTERNS,
    MimeResult,
};

static DEFAULT_MIME_CONFIG: OnceLock<RwLock<MimeConfig>> = OnceLock::new();

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

    /// Creates a configuration from explicit values.
    ///
    /// # Parameters
    /// - `mime_detector_default`: Default MIME detector selector.
    /// - `media_stream_classifier_default`: Default media stream classifier selector.
    /// - `enable_precise_detection`: Whether precise detection is enabled.
    /// - `precise_detection_patterns`: Comma-separated extension list.
    /// - `ambiguous_mime_mapping`: Semicolon-separated ambiguous mapping list.
    ///
    /// # Returns
    /// Parsed configuration.
    pub fn new(
        mime_detector_default: &str,
        media_stream_classifier_default: &str,
        enable_precise_detection: bool,
        precise_detection_patterns: &str,
        ambiguous_mime_mapping: &str,
    ) -> Self {
        Self {
            mime_detector_default: normalize_selector(mime_detector_default, DEFAULT_MIME_DETECTOR),
            media_stream_classifier_default: normalize_selector(
                media_stream_classifier_default,
                DEFAULT_MEDIA_STREAM_CLASSIFIER,
            ),
            enable_precise_detection,
            precise_detection_patterns: parse_patterns(precise_detection_patterns),
            ambiguous_mime_mapping: parse_mapping(ambiguous_mime_mapping),
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
    /// configuration value cannot be read as text.
    pub fn from_config(config: &Config) -> MimeResult<Self> {
        let mime_detector_default = read_string(
            config,
            &[CONFIG_MIME_DETECTOR_DEFAULT, ENV_MIME_DETECTOR_DEFAULT],
            DEFAULT_MIME_DETECTOR,
        )?;
        let media_stream_classifier_default = read_string(
            config,
            &[
                CONFIG_MEDIA_STREAM_CLASSIFIER_DEFAULT,
                ENV_MEDIA_STREAM_CLASSIFIER_DEFAULT,
            ],
            DEFAULT_MEDIA_STREAM_CLASSIFIER,
        )?;
        let enable_precise_detection = read_bool(
            config,
            &[
                CONFIG_MIME_ENABLE_PRECISE_DETECTION,
                ENV_MIME_DETECTOR_ENABLE_PRECISE_DETECTION,
            ],
            DEFAULT_ENABLE_PRECISE_DETECTION,
        )?;
        let precise_detection_patterns = read_string(
            config,
            &[
                CONFIG_MIME_PRECISE_DETECTION_PATTERNS,
                ENV_MIME_DETECTOR_PRECISE_DETECTION_PATTERNS,
            ],
            DEFAULT_PRECISE_DETECTION_PATTERNS,
        )?;
        let ambiguous_mime_mapping = read_string(
            config,
            &[
                CONFIG_MIME_AMBIGUOUS_MIME_MAPPING,
                ENV_MIME_DETECTOR_AMBIGUOUS_MIME_MAPPING,
            ],
            DEFAULT_AMBIGUOUS_MIME_MAPPING,
        )?;
        Ok(Self::new(
            &mime_detector_default,
            &media_stream_classifier_default,
            enable_precise_detection,
            &precise_detection_patterns,
            &ambiguous_mime_mapping,
        ))
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
        let cell = default_config_cell();
        match cell.write() {
            Ok(mut guard) => *guard = config,
            Err(poisoned) => *poisoned.into_inner() = config,
        }
    }

    /// Reloads the global default MIME configuration from a config object.
    ///
    /// # Parameters
    /// - `config`: Configuration object used to build the new default.
    ///
    /// # Errors
    /// Returns [`MimeError::Config`](crate::MimeError::Config) when a present
    /// configuration value cannot be read as text.
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
        Self::new(
            DEFAULT_MIME_DETECTOR,
            DEFAULT_MEDIA_STREAM_CLASSIFIER,
            DEFAULT_ENABLE_PRECISE_DETECTION,
            DEFAULT_PRECISE_DETECTION_PATTERNS,
            DEFAULT_AMBIGUOUS_MIME_MAPPING,
        )
    }

    /// Creates configuration from raw optional values.
    ///
    /// # Parameters
    /// - `mime_detector_default`: Optional MIME detector selector.
    /// - `media_stream_classifier_default`: Optional media stream classifier selector.
    /// - `enable_precise_detection`: Optional boolean text.
    /// - `precise_detection_patterns`: Optional comma-separated pattern text.
    /// - `ambiguous_mime_mapping`: Optional semicolon-separated mapping text.
    ///
    /// # Returns
    /// Parsed configuration with defaults for missing or invalid values.
    #[cfg(coverage)]
    fn from_raw_values(
        mime_detector_default: Option<String>,
        media_stream_classifier_default: Option<String>,
        enable_precise_detection: Option<String>,
        precise_detection_patterns: Option<String>,
        ambiguous_mime_mapping: Option<String>,
    ) -> Self {
        let mime_detector_default =
            mime_detector_default.unwrap_or_else(|| DEFAULT_MIME_DETECTOR.to_owned());
        let media_stream_classifier_default = media_stream_classifier_default
            .unwrap_or_else(|| DEFAULT_MEDIA_STREAM_CLASSIFIER.to_owned());
        let enable_precise_detection = enable_precise_detection
            .as_deref()
            .and_then(parse_bool)
            .unwrap_or(DEFAULT_ENABLE_PRECISE_DETECTION);
        let precise_detection_patterns = precise_detection_patterns
            .unwrap_or_else(|| DEFAULT_PRECISE_DETECTION_PATTERNS.to_owned());
        let ambiguous_mime_mapping =
            ambiguous_mime_mapping.unwrap_or_else(|| DEFAULT_AMBIGUOUS_MIME_MAPPING.to_owned());
        Self::new(
            &mime_detector_default,
            &media_stream_classifier_default,
            enable_precise_detection,
            &precise_detection_patterns,
            &ambiguous_mime_mapping,
        )
    }
}

impl Default for MimeConfig {
    /// Loads default configuration.
    fn default() -> Self {
        let cell = default_config_cell();
        match cell.read() {
            Ok(guard) => guard.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }
}

/// Gets the global default configuration cell.
///
/// # Returns
/// Shared lock containing the current default MIME configuration.
fn default_config_cell() -> &'static RwLock<MimeConfig> {
    DEFAULT_MIME_CONFIG.get_or_init(|| RwLock::new(MimeConfig::load()))
}

/// Reads a string from the first present configuration key.
///
/// # Parameters
/// - `config`: Configuration object.
/// - `keys`: Candidate keys in priority order.
/// - `default`: Fallback value when none of the keys are present.
///
/// # Returns
/// Configured string or fallback.
///
/// # Errors
/// Returns configuration errors from `config` when a present value cannot be
/// read as a string.
fn read_string(config: &Config, keys: &[&str], default: &str) -> MimeResult<String> {
    for key in keys {
        if let Some(value) = config.get_optional_string(key)? {
            let value = value.trim();
            if !value.is_empty() {
                return Ok(value.to_owned());
            }
        }
    }
    Ok(default.to_owned())
}

/// Reads a permissive boolean from the first present configuration key.
///
/// # Parameters
/// - `config`: Configuration object.
/// - `keys`: Candidate keys in priority order.
/// - `default`: Fallback value when none of the keys parse as a boolean.
///
/// # Returns
/// Configured boolean or fallback.
///
/// # Errors
/// Returns configuration errors from `config` when a present value cannot be
/// read as a string.
fn read_bool(config: &Config, keys: &[&str], default: bool) -> MimeResult<bool> {
    for key in keys {
        if let Some(value) = config.get_optional_string(key)?
            && let Some(parsed) = parse_bool(&value)
        {
            return Ok(parsed);
        }
    }
    Ok(default)
}

/// Normalizes a backend selector with a fallback for empty values.
///
/// # Parameters
/// - `selector`: Raw selector text.
/// - `default`: Fallback selector for empty values.
///
/// # Returns
/// Trimmed selector or fallback.
fn normalize_selector(selector: &str, default: &str) -> String {
    let selector = selector.trim();
    if selector.is_empty() {
        default.to_owned()
    } else {
        selector.to_owned()
    }
}

/// Parses a permissive boolean environment value.
///
/// # Parameters
/// - `value`: Text to parse.
///
/// # Returns
/// Parsed boolean, or `None` when the value is not recognized.
fn parse_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

/// Parses comma-separated extension patterns.
///
/// # Parameters
/// - `patterns`: Comma-separated extension text.
///
/// # Returns
/// Lowercase extension set.
fn parse_patterns(patterns: &str) -> HashSet<String> {
    patterns
        .split(',')
        .map(str::trim)
        .filter(|pattern| !pattern.is_empty())
        .map(|pattern| pattern.trim_start_matches('.').to_ascii_lowercase())
        .collect()
}

/// Parses ambiguous MIME mapping text.
///
/// # Parameters
/// - `mapping`: Mapping text in `ext:video,audio;...` format.
///
/// # Returns
/// Lowercase extension to MIME pair mapping.
fn parse_mapping(mapping: &str) -> HashMap<String, [String; 2]> {
    mapping
        .split(';')
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

#[cfg(coverage)]
pub(crate) mod coverage_support {
    //! Coverage helpers for configuration parsing branches.

    use qubit_config::Config;

    use super::{MimeConfig, parse_bool, parse_mapping, parse_patterns};
    use crate::{
        CONFIG_MEDIA_STREAM_CLASSIFIER_DEFAULT, CONFIG_MIME_AMBIGUOUS_MIME_MAPPING,
        CONFIG_MIME_DETECTOR_DEFAULT, CONFIG_MIME_ENABLE_PRECISE_DETECTION,
        CONFIG_MIME_PRECISE_DETECTION_PATTERNS,
    };

    /// Exercises explicit and default configuration parsing.
    ///
    /// # Returns
    /// Summary strings for parsed configuration values.
    pub(crate) fn exercise_config_edges() -> Vec<String> {
        let config = MimeConfig::new(
            "repository",
            "ffprobe",
            false,
            "webm,.ogg,, ",
            "webm:video/webm,audio/webm;bad;ogg:video/ogg,audio/ogg;bad:one;empty:,audio/x;extra:video/x,audio/x,other",
        );
        let blank_selectors = MimeConfig::new(" ", "\t", false, "", "");
        let mut explicit_config = Config::new();
        explicit_config
            .set(CONFIG_MIME_DETECTOR_DEFAULT, "repository")
            .expect("detector default should be configurable");
        explicit_config
            .set(CONFIG_MEDIA_STREAM_CLASSIFIER_DEFAULT, "ffprobe")
            .expect("classifier default should be configurable");
        explicit_config
            .set(CONFIG_MIME_ENABLE_PRECISE_DETECTION, "yes")
            .expect("precise detection should be configurable");
        explicit_config
            .set(CONFIG_MIME_PRECISE_DETECTION_PATTERNS, "webm")
            .expect("precise detection patterns should be configurable");
        explicit_config
            .set(
                CONFIG_MIME_AMBIGUOUS_MIME_MAPPING,
                "webm:video/webm,audio/webm",
            )
            .expect("ambiguous MIME mapping should be configurable");
        let explicit = MimeConfig::from_config(&explicit_config)
            .expect("explicit coverage config should parse");
        let builtin = MimeConfig::builtin_default();
        let loaded = MimeConfig::load();
        let defaulted = <MimeConfig as Default>::default();
        let raw_values = MimeConfig::from_raw_values(
            Some("repository".to_owned()),
            Some("ffprobe".to_owned()),
            Some("true".to_owned()),
            Some("webm".to_owned()),
            Some("webm:video/webm,audio/webm".to_owned()),
        );
        let raw_invalid =
            MimeConfig::from_raw_values(None, None, Some("maybe".to_owned()), None, None);
        vec![
            config.mime_detector_default().to_owned(),
            config.media_stream_classifier_default().to_owned(),
            config.enable_precise_detection().to_string(),
            blank_selectors.mime_detector_default().to_owned(),
            blank_selectors.media_stream_classifier_default().to_owned(),
            explicit.enable_precise_detection().to_string(),
            explicit
                .ambiguous_mime_mapping()
                .contains_key("webm")
                .to_string(),
            builtin.mime_detector_default().to_owned(),
            config
                .precise_detection_patterns()
                .contains("ogg")
                .to_string(),
            config.ambiguous_mime_mapping().len().to_string(),
            loaded.enable_precise_detection().to_string(),
            defaulted.enable_precise_detection().to_string(),
            raw_values.enable_precise_detection().to_string(),
            raw_invalid.enable_precise_detection().to_string(),
            format!("{:?}", parse_bool("yes")),
            format!("{:?}", parse_bool("true")),
            format!("{:?}", parse_bool("1")),
            format!("{:?}", parse_bool("on")),
            format!("{:?}", parse_bool("off")),
            format!("{:?}", parse_bool("false")),
            format!("{:?}", parse_bool("0")),
            format!("{:?}", parse_bool("no")),
            format!("{:?}", parse_bool("maybe")),
            parse_patterns("a,.b").len().to_string(),
            parse_mapping("x:video/x,audio/x;y:video/y,audio/y")
                .len()
                .to_string(),
        ]
    }
}
