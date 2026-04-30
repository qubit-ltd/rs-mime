/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Configuration values for MIME detection.
//!
//! # Author
//!
//! Haixing Hu

use std::collections::{HashMap, HashSet};
use std::env;

use crate::{
    DEFAULT_AMBIGUOUS_MIME_MAPPING, DEFAULT_ENABLE_PRECISE_DETECTION,
    DEFAULT_PRECISE_DETECTION_PATTERNS, ENV_MIME_DETECTOR_AMBIGUOUS_MIME_MAPPING,
    ENV_MIME_DETECTOR_ENABLE_PRECISE_DETECTION, ENV_MIME_DETECTOR_PRECISE_DETECTION_PATTERNS,
};

/// Runtime configuration for MIME detectors.
#[derive(Debug, Clone)]
pub struct MimeConfig {
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
        Self::from_raw_values(
            env::var(ENV_MIME_DETECTOR_ENABLE_PRECISE_DETECTION).ok(),
            env::var(ENV_MIME_DETECTOR_PRECISE_DETECTION_PATTERNS).ok(),
            env::var(ENV_MIME_DETECTOR_AMBIGUOUS_MIME_MAPPING).ok(),
        )
    }

    /// Creates a configuration from explicit values.
    ///
    /// # Parameters
    /// - `enable_precise_detection`: Whether precise detection is enabled.
    /// - `precise_detection_patterns`: Comma-separated extension list.
    /// - `ambiguous_mime_mapping`: Semicolon-separated ambiguous mapping list.
    ///
    /// # Returns
    /// Parsed configuration.
    pub fn new(
        enable_precise_detection: bool,
        precise_detection_patterns: &str,
        ambiguous_mime_mapping: &str,
    ) -> Self {
        Self {
            enable_precise_detection,
            precise_detection_patterns: parse_patterns(precise_detection_patterns),
            ambiguous_mime_mapping: parse_mapping(ambiguous_mime_mapping),
        }
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

    /// Creates configuration from raw optional environment values.
    ///
    /// # Parameters
    /// - `enable_precise_detection`: Optional boolean text.
    /// - `precise_detection_patterns`: Optional comma-separated pattern text.
    /// - `ambiguous_mime_mapping`: Optional semicolon-separated mapping text.
    ///
    /// # Returns
    /// Parsed configuration with defaults for missing or invalid values.
    fn from_raw_values(
        enable_precise_detection: Option<String>,
        precise_detection_patterns: Option<String>,
        ambiguous_mime_mapping: Option<String>,
    ) -> Self {
        let enable_precise_detection = enable_precise_detection
            .as_deref()
            .and_then(parse_bool)
            .unwrap_or(DEFAULT_ENABLE_PRECISE_DETECTION);
        let precise_detection_patterns = precise_detection_patterns
            .unwrap_or_else(|| DEFAULT_PRECISE_DETECTION_PATTERNS.to_owned());
        let ambiguous_mime_mapping =
            ambiguous_mime_mapping.unwrap_or_else(|| DEFAULT_AMBIGUOUS_MIME_MAPPING.to_owned());
        Self::new(
            enable_precise_detection,
            &precise_detection_patterns,
            &ambiguous_mime_mapping,
        )
    }
}

impl Default for MimeConfig {
    /// Loads default configuration.
    fn default() -> Self {
        Self::load()
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

    use super::{MimeConfig, parse_bool, parse_mapping, parse_patterns};

    /// Exercises explicit and default configuration parsing.
    ///
    /// # Returns
    /// Summary strings for parsed configuration values.
    pub(crate) fn exercise_config_edges() -> Vec<String> {
        let config = MimeConfig::new(
            false,
            "webm,.ogg,, ",
            "webm:video/webm,audio/webm;bad;ogg:video/ogg,audio/ogg;bad:one;empty:,audio/x;extra:video/x,audio/x,other",
        );
        let loaded = MimeConfig::load();
        let defaulted = <MimeConfig as Default>::default();
        let raw_values = MimeConfig::from_raw_values(
            Some("true".to_owned()),
            Some("webm".to_owned()),
            Some("webm:video/webm,audio/webm".to_owned()),
        );
        let raw_invalid = MimeConfig::from_raw_values(Some("maybe".to_owned()), None, None);
        vec![
            config.enable_precise_detection().to_string(),
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
