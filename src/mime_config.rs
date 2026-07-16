// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Configuration values for MIME detection.
//!
//! [`MimeConfig`] is the runtime configuration shared by detector wrappers,
//! detector providers, and media-stream refinement. It can be loaded from a
//! [`Config`] object, from process environment variables, or from built-in
//! defaults.

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
use qubit_spi::ProviderSelection;
use qubit_spi::error::ProviderSelectionError;

use crate::MimeError;
use crate::{
    CONFIG_MEDIA_STREAM_CLASSIFIER_DEFAULT,
    CONFIG_MEDIA_STREAM_MAX_STAGING_SIZE,
    CONFIG_MIME_AMBIGUOUS_MIME_MAPPING,
    CONFIG_MIME_DETECTOR_DEFAULT,
    CONFIG_MIME_DETECTOR_FALLBACKS,
    CONFIG_MIME_ENABLE_PRECISE_DETECTION,
    CONFIG_MIME_MAX_BUFFER_SIZE,
    CONFIG_MIME_PRECISE_DETECTION_PATTERNS,
    DEFAULT_ENABLE_PRECISE_DETECTION,
    DEFAULT_MEDIA_STREAM_CLASSIFIER,
    DEFAULT_MEDIA_STREAM_MAX_STAGING_SIZE,
    DEFAULT_MIME_DETECTOR,
    DEFAULT_MIME_DETECTOR_FALLBACKS,
    DEFAULT_MIME_MAX_BUFFER_SIZE,
    ENV_MEDIA_STREAM_CLASSIFIER_DEFAULT,
    ENV_MEDIA_STREAM_MAX_STAGING_SIZE,
    ENV_MIME_DETECTOR_AMBIGUOUS_MIME_MAPPING,
    ENV_MIME_DETECTOR_DEFAULT,
    ENV_MIME_DETECTOR_ENABLE_PRECISE_DETECTION,
    ENV_MIME_DETECTOR_FALLBACKS,
    ENV_MIME_DETECTOR_PRECISE_DETECTION_PATTERNS,
    ENV_MIME_MAX_BUFFER_SIZE,
    MimeResult,
};

/// Runtime configuration for MIME detectors.
///
/// # Supported keys
///
/// Logical keys and environment-style keys are both accepted by
/// [`MimeConfig::from_config`]. Environment variables use the same names as the
/// environment-style keys.
///
/// | Field | Logical key | Environment key | Default | Format |
/// | --- | --- | --- | --- | --- |
/// | Default MIME detector | `mime.detector.default` | `QUBIT_MIME_DETECTOR_DEFAULT` | `repository` | Provider id, alias, or `auto` |
/// | MIME detector fallbacks | `mime.detector.fallbacks` | `QUBIT_MIME_DETECTOR_FALLBACKS` | empty | List split on `,` or `;` |
/// | Media stream classifier | `mime.media.stream.classifier.default` | `QUBIT_MEDIA_STREAM_CLASSIFIER_DEFAULT` | `ffprobe` | Classifier selector |
/// | Media stream staging limit | `mime.media.stream.max.staging.size` | `QUBIT_MEDIA_STREAM_MAX_STAGING_SIZE` | `67108864` | Byte count |
/// | Precise detection switch | `mime.enable.precise.detection` | `QUBIT_MIME_ENABLE_PRECISE_DETECTION` | `true` | Boolean |
/// | Precise detection patterns | `mime.precise.detection.patterns` | `QUBIT_MIME_PRECISE_DETECTION_PATTERNS` | `webm,ogg` | Extension list |
/// | Ambiguous MIME mapping | `mime.ambiguous.mime.mapping` | `QUBIT_MIME_AMBIGUOUS_MIME_MAPPING` | `webm:video/webm,audio/webm;ogg:video/ogg,audio/ogg` | `ext:video,audio` entries split on `;` |
/// | Maximum detector buffer size | `mime.max.buffer.size` | `QUBIT_MIME_MAX_BUFFER_SIZE` | `16777216` | Byte count |
///
/// Detector fallback selection is performed by
/// [`MimeDetectorRegistry`](crate::MimeDetectorRegistry), not by this config
/// object. Selector text is validated while configuration is loaded and stored
/// as reusable [`ProviderSelection`] values.
#[derive(Debug, Clone)]
pub struct MimeConfig {
    /// Validated MIME detector selection.
    mime_detector_selection: ProviderSelection,
    /// Validated media stream classifier selection.
    media_stream_classifier_selection: ProviderSelection,
    /// Maximum bytes staged from reader/content input for media stream
    /// classification.
    media_stream_max_staging_size: u64,
    /// Whether precise media-stream detection is enabled.
    enable_precise_detection: bool,
    /// Extensions requiring precise detection.
    precise_detection_patterns: HashSet<String>,
    /// Ambiguous MIME mappings.
    ambiguous_mime_mapping: HashMap<String, [String; 2]>,
    /// Maximum byte buffer size used by detector read paths.
    max_buffer_size: usize,
}

/// Default MIME configuration.
static DEFAULT_MIME_CONFIG: LazyLock<RwLock<MimeConfig>> =
    LazyLock::new(|| RwLock::new(MimeConfig::load()));

/// Value read options.
static VALUE_READ_OPTIONS: LazyLock<ConfigReadOptions> =
    LazyLock::new(ConfigReadOptions::env_friendly);

/// List value read options.
static LIST_READ_OPTIONS: LazyLock<ConfigReadOptions> = LazyLock::new(|| {
    ConfigReadOptions::env_friendly().with_collection_options(
        CollectionReadOptions::default()
            .with_split_scalar_strings(true)
            .with_delimiters([',', ';'])
            .with_trim_items(true)
            .with_empty_item_policy(EmptyItemPolicy::Skip),
    )
});

/// Mapping read options.
static MAPPING_READ_OPTIONS: LazyLock<ConfigReadOptions> =
    LazyLock::new(|| {
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
    /// Values are read with environment-friendly options, so both logical keys
    /// such as `mime.detector.default` and environment-style keys such as
    /// `QUBIT_MIME_DETECTOR_DEFAULT` are accepted. List values may be provided
    /// as arrays or as scalar strings split on `,` and `;`; empty items are
    /// ignored.
    ///
    /// # Examples
    ///
    /// Configure a preferred native detector and a repository fallback:
    ///
    /// ```rust
    /// use qubit_config::Config;
    /// use qubit_mime::{
    ///     CONFIG_MIME_DETECTOR_DEFAULT,
    ///     CONFIG_MIME_DETECTOR_FALLBACKS,
    ///     MimeConfig,
    ///     MimeResult,
    /// };
    ///
    /// # fn main() -> MimeResult<()> {
    /// let mut source = Config::new();
    /// source.set(CONFIG_MIME_DETECTOR_DEFAULT, "file")?;
    /// source.set(CONFIG_MIME_DETECTOR_FALLBACKS, "repository")?;
    ///
    /// let config = MimeConfig::from_config(&source)?;
    /// assert_eq!(
    ///     "file",
    ///     config.mime_detector_selection().selectors()[0].as_str(),
    /// );
    /// assert_eq!(
    ///     "repository",
    ///     config.mime_detector_selection().selectors()[1].as_str(),
    /// );
    /// # Ok(())
    /// # }
    /// ```
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
    /// configuration value cannot be converted to the expected type. Returns
    /// a detector- or classifier-name error when a configured provider
    /// selector is invalid.
    pub fn from_config(config: &Config) -> MimeResult<Self> {
        let mime_detector_default = config.get_any_or_with(
            [CONFIG_MIME_DETECTOR_DEFAULT, ENV_MIME_DETECTOR_DEFAULT],
            DEFAULT_MIME_DETECTOR.to_owned(),
            &VALUE_READ_OPTIONS,
        )?;
        let mime_detector_fallbacks = config.get_any_or_with(
            [CONFIG_MIME_DETECTOR_FALLBACKS, ENV_MIME_DETECTOR_FALLBACKS],
            fallback_defaults(),
            &LIST_READ_OPTIONS,
        )?;
        let media_stream_classifier_default = config.get_any_or_with(
            [
                CONFIG_MEDIA_STREAM_CLASSIFIER_DEFAULT,
                ENV_MEDIA_STREAM_CLASSIFIER_DEFAULT,
            ],
            DEFAULT_MEDIA_STREAM_CLASSIFIER.to_owned(),
            &VALUE_READ_OPTIONS,
        )?;
        let media_stream_max_staging_size = config.get_any_or_with(
            [
                CONFIG_MEDIA_STREAM_MAX_STAGING_SIZE,
                ENV_MEDIA_STREAM_MAX_STAGING_SIZE,
            ],
            DEFAULT_MEDIA_STREAM_MAX_STAGING_SIZE,
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
        let max_buffer_size: u64 = config.get_any_or_with(
            [CONFIG_MIME_MAX_BUFFER_SIZE, ENV_MIME_MAX_BUFFER_SIZE],
            DEFAULT_MIME_MAX_BUFFER_SIZE as u64,
            &VALUE_READ_OPTIONS,
        )?;
        #[cfg(target_pointer_width = "32")]
        let max_buffer_size =
            usize::try_from(max_buffer_size).map_err(|_| {
                MimeError::InvalidClassifierInput {
                    reason: format!(
                        "MIME maximum buffer size {max_buffer_size} exceeds this platform's usize range"
                    ),
                }
            })?;
        #[cfg(target_pointer_width = "64")]
        let max_buffer_size = max_buffer_size as usize;
        let mime_detector_selection = create_detector_selection(
            &mime_detector_default,
            normalize_detector_names(mime_detector_fallbacks),
        )?;
        let media_stream_classifier_selection =
            create_classifier_selection(&media_stream_classifier_default)?;
        Ok(Self {
            mime_detector_selection,
            media_stream_classifier_selection,
            media_stream_max_staging_size,
            enable_precise_detection,
            precise_detection_patterns: normalize_patterns(
                precise_detection_patterns,
            ),
            ambiguous_mime_mapping: build_ambiguous_mime_mapping(
                ambiguous_mime_mapping,
            ),
            max_buffer_size,
        })
    }

    /// Creates MIME configuration from process environment variables.
    ///
    /// # Returns
    /// Parsed MIME configuration.
    ///
    /// # Errors
    /// Returns [`MimeError::Config`](crate::MimeError::Config) when the
    /// environment cannot be represented by [`Config`]. Returns a detector-
    /// or classifier-name error when a configured provider selector is
    /// invalid.
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
    /// configuration value cannot be converted to the expected type. Returns
    /// a detector- or classifier-name error when a configured provider
    /// selector is invalid.
    pub fn reload_default(config: &Config) -> MimeResult<()> {
        Self::set_default(Self::from_config(config)?);
        Ok(())
    }

    /// Reloads the global default MIME configuration from process environment.
    ///
    /// # Errors
    /// Returns [`MimeError::Config`](crate::MimeError::Config) when the
    /// environment cannot be represented by [`Config`]. Returns a detector-
    /// or classifier-name error when a configured provider selector is
    /// invalid.
    pub fn reload_default_from_env() -> MimeResult<()> {
        Self::set_default(Self::from_env()?);
        Ok(())
    }

    /// Returns the validated MIME detector selection.
    ///
    /// # Returns
    /// Automatic, named, or chained selection used by detector registries.
    #[inline(always)]
    #[must_use]
    pub const fn mime_detector_selection(&self) -> &ProviderSelection {
        &self.mime_detector_selection
    }

    /// Returns the validated media stream classifier selection.
    ///
    /// # Returns
    /// Automatic or named selection used by classifier registries.
    #[inline(always)]
    #[must_use]
    pub const fn media_stream_classifier_selection(
        &self,
    ) -> &ProviderSelection {
        &self.media_stream_classifier_selection
    }

    /// Gets the maximum staging size for reader/content media stream
    /// classification.
    ///
    /// # Returns
    /// Maximum number of bytes copied to a temporary file for one classifier
    /// input.
    pub fn media_stream_max_staging_size(&self) -> u64 {
        self.media_stream_max_staging_size
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

    /// Gets the maximum byte buffer size allowed for detector read paths.
    ///
    /// # Returns
    /// Maximum number of bytes a detector may allocate for one read buffer.
    pub fn max_buffer_size(&self) -> usize {
        self.max_buffer_size
    }

    /// Creates the built-in MIME configuration.
    ///
    /// # Returns
    /// Configuration populated entirely from crate constants.
    fn builtin_default() -> Self {
        Self {
            mime_detector_selection: create_detector_selection(
                DEFAULT_MIME_DETECTOR,
                fallback_defaults(),
            )
            .expect("built-in MIME detector selection should be valid"),
            media_stream_classifier_selection: create_classifier_selection(
                DEFAULT_MEDIA_STREAM_CLASSIFIER,
            )
            .expect(
                "built-in media stream classifier selection should be valid",
            ),
            media_stream_max_staging_size:
                DEFAULT_MEDIA_STREAM_MAX_STAGING_SIZE,
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
            max_buffer_size: DEFAULT_MIME_MAX_BUFFER_SIZE,
        }
    }
}

/// Builds the validated MIME detector selection from configured text.
///
/// # Arguments
///
/// * `primary` - Configured primary selector or `auto` sentinel.
/// * `fallbacks` - Normalized fallback selectors in configured order.
///
/// # Returns
///
/// The validated automatic or chained detector selection.
///
/// # Errors
///
/// Returns a detector-name error when any configured selector is invalid.
fn create_detector_selection(
    primary: &str,
    fallbacks: Vec<String>,
) -> MimeResult<ProviderSelection> {
    let primary = primary.trim();
    if primary.is_empty() || primary.eq_ignore_ascii_case("auto") {
        return Ok(ProviderSelection::auto());
    }
    ProviderSelection::chain(
        std::iter::once(primary).chain(fallbacks.iter().map(String::as_str)),
    )
    .map_err(detector_selection_error)
}

/// Builds the validated media stream classifier selection.
///
/// # Arguments
///
/// * `configured` - Configured selector or `auto` sentinel.
///
/// # Returns
///
/// The validated automatic or named classifier selection.
///
/// # Errors
///
/// Returns a classifier-name error when the configured selector is invalid.
fn create_classifier_selection(
    configured: &str,
) -> MimeResult<ProviderSelection> {
    let configured = configured.trim();
    if configured.is_empty() || configured.eq_ignore_ascii_case("auto") {
        return Ok(ProviderSelection::auto());
    }
    ProviderSelection::named(configured).map_err(classifier_selection_error)
}

/// Maps detector selection validation into the MIME error model.
///
/// # Arguments
///
/// * `error` - Invalid selector or empty-chain error.
///
/// # Returns
///
/// A detector-specific configuration error.
fn detector_selection_error(error: ProviderSelectionError) -> MimeError {
    let reason = error.to_string();
    match error {
        ProviderSelectionError::InvalidSelector { source, .. } => {
            MimeError::InvalidDetectorName {
                name: source.input().to_owned(),
                reason,
            }
        }
        ProviderSelectionError::EmptyChain => MimeError::EmptyDetectorName,
        _ => MimeError::NoAvailableDetector { reason },
    }
}

/// Maps classifier selection validation into the MIME error model.
///
/// # Arguments
///
/// * `error` - Invalid selector error.
///
/// # Returns
///
/// A classifier-specific configuration error.
fn classifier_selection_error(error: ProviderSelectionError) -> MimeError {
    let reason = error.to_string();
    match error {
        ProviderSelectionError::InvalidSelector { source, .. } => {
            MimeError::InvalidClassifierName {
                name: source.input().to_owned(),
                reason,
            }
        }
        ProviderSelectionError::EmptyChain => MimeError::EmptyClassifierName,
        _ => MimeError::NoAvailableClassifier { reason },
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

/// Gets built-in fallback detector defaults.
///
/// # Returns
/// Default fallback detector names.
fn fallback_defaults() -> Vec<String> {
    DEFAULT_MIME_DETECTOR_FALLBACKS
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Normalizes detector names read from configuration.
///
/// # Parameters
/// - `names`: Raw detector names.
///
/// # Returns
/// Trimmed detector names with empty values removed.
fn normalize_detector_names(names: Vec<String>) -> Vec<String> {
    names
        .into_iter()
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
        .collect()
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
fn build_ambiguous_mime_mapping(
    entries: Vec<String>,
) -> HashMap<String, [String; 2]> {
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
