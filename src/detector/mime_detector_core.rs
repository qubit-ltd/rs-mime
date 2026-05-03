/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Shared MIME detector behavior.

use std::path::Path;

use crate::{
    ArcMediaStreamClassifier, MediaStreamClassifier, MediaStreamType, MimeConfig,
    MimeDetectionPolicy,
};

use super::detection_source::DetectionSource;

/// Shared detector core for configuration and merge/refinement logic.
#[derive(Debug, Clone)]
pub struct MimeDetectorCore {
    /// MIME detector configuration.
    config: MimeConfig,
    /// Media stream classifier.
    media_stream_classifier: Option<ArcMediaStreamClassifier>,
}

impl MimeDetectorCore {
    /// Creates a detector core from configuration.
    ///
    /// # Parameters
    /// - `config`: MIME detector configuration.
    ///
    /// # Returns
    /// Shared detector core.
    pub fn new(config: MimeConfig) -> Self {
        Self {
            config,
            media_stream_classifier: None,
        }
    }

    /// Creates a detector core from configuration and default classifier.
    ///
    /// # Parameters
    /// - `config`: MIME detector configuration.
    ///
    /// # Returns
    /// Shared detector core using the configured default media classifier when
    /// precise detection is enabled.
    pub fn from_mime_config(config: MimeConfig) -> Self {
        let mut detector = Self::new(config.clone());
        if config.enable_precise_detection() {
            let classifier = ArcMediaStreamClassifier::from_config(&config);
            detector.set_media_stream_classifier(Some(classifier));
        }
        detector
    }

    /// Sets the classifier used for precise media MIME refinement.
    ///
    /// # Parameters
    /// - `media_stream_classifier`: Classifier to use, or `None` to disable
    ///   runtime media stream refinement.
    pub fn set_media_stream_classifier(
        &mut self,
        media_stream_classifier: Option<ArcMediaStreamClassifier>,
    ) {
        self.media_stream_classifier = media_stream_classifier;
    }

    /// Gets the classifier used for precise media MIME refinement.
    ///
    /// # Returns
    /// Configured classifier, or `None`.
    pub fn media_stream_classifier(&self) -> Option<&ArcMediaStreamClassifier> {
        self.media_stream_classifier.as_ref()
    }

    /// Merges filename and content candidates using the detector selection strategy.
    ///
    /// # Parameters
    /// - `from_filename`: Candidates from filename glob detection.
    /// - `from_content`: Candidates from content magic detection.
    ///
    /// # Returns
    /// Selected MIME type name, or `None`.
    pub fn merge_results(
        &self,
        from_filename: &[String],
        from_content: &[String],
    ) -> Option<String> {
        if from_filename.is_empty() {
            return from_content.first().cloned();
        }
        if from_content.is_empty() {
            return from_filename.first().cloned();
        }
        from_filename
            .iter()
            .find(|candidate| from_content.contains(candidate))
            .cloned()
            .or_else(|| from_content.first().cloned())
    }

    /// Selects a MIME type from filename/content candidates and refines it.
    ///
    /// # Parameters
    /// - `from_filename`: Candidates from filename glob detection.
    /// - `from_content`: Candidates from content magic detection.
    /// - `filename`: Optional filename used for precise detection decisions.
    /// - `policy`: Strategy for resolving filename and content results.
    /// - `source`: Source available for optional media stream refinement.
    ///
    /// # Returns
    /// Selected and optionally refined MIME type name.
    pub fn select_result(
        &self,
        from_filename: &[String],
        from_content: &[String],
        filename: Option<&str>,
        policy: MimeDetectionPolicy,
        source: DetectionSource<'_>,
    ) -> Option<String> {
        let result = if from_filename.len() == 1 && policy == MimeDetectionPolicy::PreferFilename {
            from_filename.first().cloned()
        } else {
            self.merge_results(from_filename, from_content)
        }?;
        Some(self.refine_detected_mime_type(&result, filename, source))
    }

    /// Refines an ambiguous media MIME type using a stream classifier.
    ///
    /// # Parameters
    /// - `detected_mime_type`: Initial MIME type name.
    /// - `filename`: Optional filename used to resolve the ambiguous mapping.
    /// - `source`: Source to classify.
    ///
    /// # Returns
    /// Refined MIME type name, or the original type if refinement is disabled
    /// or cannot be performed.
    pub fn refine_detected_mime_type(
        &self,
        detected_mime_type: &str,
        filename: Option<&str>,
        source: DetectionSource<'_>,
    ) -> String {
        let Some([video_type, audio_type]) =
            self.precise_detection_mapping(detected_mime_type, filename)
        else {
            return detected_mime_type.to_owned();
        };
        let Some(classifier) = &self.media_stream_classifier else {
            return detected_mime_type.to_owned();
        };
        let stream_type = match source {
            DetectionSource::Content(content) => classifier.classify_content(content),
            DetectionSource::Path(path) => classifier.classify_file(path),
            DetectionSource::None => return detected_mime_type.to_owned(),
        };
        match stream_type.unwrap_or(MediaStreamType::None) {
            MediaStreamType::AudioOnly => audio_type.clone(),
            MediaStreamType::VideoOnly | MediaStreamType::VideoWithAudio => video_type.clone(),
            MediaStreamType::None => detected_mime_type.to_owned(),
        }
    }

    /// Gets the ambiguous mapping pair for a MIME type and optional filename.
    ///
    /// # Parameters
    /// - `detected_mime_type`: Initial MIME type name.
    /// - `filename`: Optional filename.
    ///
    /// # Returns
    /// Video/audio MIME pair when precise detection should run.
    fn precise_detection_mapping(
        &self,
        detected_mime_type: &str,
        filename: Option<&str>,
    ) -> Option<&[String; 2]> {
        let mapping_key = self.precise_detection_mapping_key(detected_mime_type, filename)?;
        self.config.ambiguous_mime_mapping().get(&mapping_key)
    }

    /// Gets the ambiguous mapping key for a MIME type and optional filename.
    ///
    /// # Parameters
    /// - `detected_mime_type`: Initial MIME type name.
    /// - `filename`: Optional filename.
    ///
    /// # Returns
    /// Extension mapping key when precise detection should run.
    fn precise_detection_mapping_key(
        &self,
        detected_mime_type: &str,
        filename: Option<&str>,
    ) -> Option<String> {
        if !self.config.enable_precise_detection() || detected_mime_type.is_empty() {
            return None;
        }
        if let Some(filename) = filename
            && let Some(extension) = extension_from_filename(filename)
        {
            return self.precise_detection_mapping_key_by_filename(detected_mime_type, extension);
        }
        self.precise_detection_mapping_key_by_mime_type(detected_mime_type)
    }

    /// Gets an ambiguous mapping key by filename extension.
    ///
    /// # Parameters
    /// - `detected_mime_type`: Initial MIME type name.
    /// - `extension`: Lowercase extension candidate.
    ///
    /// # Returns
    /// Mapping key when the extension and MIME type are ambiguous.
    fn precise_detection_mapping_key_by_filename(
        &self,
        detected_mime_type: &str,
        extension: String,
    ) -> Option<String> {
        if !self
            .config
            .precise_detection_patterns()
            .contains(&extension)
        {
            return None;
        }
        let possible_mime_types = self.config.ambiguous_mime_mapping().get(&extension)?;
        if possible_mime_types
            .iter()
            .any(|mime_type| mime_type == detected_mime_type)
        {
            Some(extension)
        } else {
            None
        }
    }

    /// Gets an ambiguous mapping key by detected MIME type.
    ///
    /// # Parameters
    /// - `detected_mime_type`: Initial MIME type name.
    ///
    /// # Returns
    /// Mapping key when the MIME type is part of an ambiguous mapping.
    fn precise_detection_mapping_key_by_mime_type(
        &self,
        detected_mime_type: &str,
    ) -> Option<String> {
        self.config
            .ambiguous_mime_mapping()
            .iter()
            .find(|(_, possible_mime_types)| {
                possible_mime_types
                    .iter()
                    .any(|mime_type| mime_type == detected_mime_type)
            })
            .map(|(extension, _)| extension.clone())
    }
}

impl Default for MimeDetectorCore {
    /// Loads the default detector core.
    fn default() -> Self {
        Self::from_mime_config(MimeConfig::default())
    }
}

/// Extracts the last extension from a filename or path.
///
/// # Parameters
/// - `filename`: Filename or path.
///
/// # Returns
/// Lowercase extension without a leading dot.
fn extension_from_filename(filename: &str) -> Option<String> {
    Path::new(filename)
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.rsplit_once('.').map(|(_, extension)| extension))
        .filter(|extension| !extension.is_empty())
        .map(str::to_ascii_lowercase)
}

// qubit-style: allow coverage-cfg
#[cfg(coverage)]
pub(crate) mod coverage_support {
    //! Coverage helpers for shared detector behavior.

    use std::io::Read;
    use std::path::Path;
    use std::sync::Arc;

    use qubit_config::Config;

    use crate::{
        ArcMediaStreamClassifier, CONFIG_MEDIA_STREAM_CLASSIFIER_DEFAULT,
        CONFIG_MIME_AMBIGUOUS_MIME_MAPPING, CONFIG_MIME_DETECTOR_DEFAULT,
        CONFIG_MIME_ENABLE_PRECISE_DETECTION, CONFIG_MIME_PRECISE_DETECTION_PATTERNS,
        MediaStreamClassifier, MediaStreamType, MimeConfig, MimeError,
    };

    use super::{DetectionSource, MimeDetectorCore, extension_from_filename};

    #[derive(Debug)]
    struct StaticClassifier {
        stream_type: MediaStreamType,
    }

    impl MediaStreamClassifier for StaticClassifier {
        /// Returns a fixed classification for any file.
        fn classify_file(&self, _file: &Path) -> crate::MimeResult<MediaStreamType> {
            Ok(self.stream_type)
        }

        /// Returns a fixed classification for any content.
        fn classify_reader(&self, _reader: &mut dyn Read) -> crate::MimeResult<MediaStreamType> {
            Ok(self.stream_type)
        }
    }

    #[derive(Debug)]
    struct FailingClassifier;

    impl MediaStreamClassifier for FailingClassifier {
        /// Fails file classification.
        fn classify_file(&self, _file: &Path) -> crate::MimeResult<MediaStreamType> {
            Err(MimeError::invalid_classifier_input("forced"))
        }

        /// Fails content classification.
        fn classify_reader(&self, _reader: &mut dyn Read) -> crate::MimeResult<MediaStreamType> {
            Err(MimeError::invalid_classifier_input("forced"))
        }
    }

    /// Exercises shared merge, precise detection, and helper branches.
    ///
    /// # Returns
    /// Summary strings from shared detector behavior.
    pub(crate) fn exercise_core_edges() -> Vec<String> {
        let mut detector = MimeDetectorCore::new(config_from_values(
            true,
            "webm,ogg",
            "webm:video/webm,audio/webm;ogg:video/ogg,audio/ogg",
        ));
        detector.set_media_stream_classifier(Some(ArcMediaStreamClassifier::new(Arc::new(
            StaticClassifier {
                stream_type: MediaStreamType::VideoOnly,
            },
        ))));
        let classifier_present = detector.media_stream_classifier().is_some().to_string();
        let from_filename = vec!["video/webm".to_owned()];
        let from_content = vec!["audio/webm".to_owned()];
        let selected = format!(
            "{:?}",
            detector.select_result(
                &from_filename,
                &from_content,
                Some("movie.webm"),
                crate::MimeDetectionPolicy::VerifyContent,
                DetectionSource::Content(b"webm"),
            )
        );
        let refined_by_type = detector.refine_detected_mime_type(
            "audio/ogg",
            None,
            DetectionSource::Path(Path::new("Cargo.toml")),
        );
        let no_source = detector.refine_detected_mime_type(
            "video/webm",
            Some("movie.webm"),
            DetectionSource::None,
        );
        let no_classifier = MimeDetectorCore::new(config_from_values(
            true,
            "webm",
            "webm:video/webm,audio/webm",
        ))
        .refine_detected_mime_type(
            "video/webm",
            Some("movie.webm"),
            DetectionSource::Content(b""),
        );
        let disabled = MimeDetectorCore::new(config_from_values(
            false,
            "webm",
            "webm:video/webm,audio/webm",
        ))
        .refine_detected_mime_type(
            "video/webm",
            Some("movie.webm"),
            DetectionSource::Content(b""),
        );
        let missing_mapping = MimeDetectorCore::new(config_from_values(true, "webm", ""))
            .refine_detected_mime_type(
                "video/webm",
                Some("movie.webm"),
                DetectionSource::Content(b""),
            );
        let mismatch = detector.refine_detected_mime_type(
            "application/pdf",
            Some("movie.webm"),
            DetectionSource::Content(b""),
        );
        let mut none_detector = MimeDetectorCore::new(config_from_values(
            true,
            "webm",
            "webm:video/webm,audio/webm",
        ));
        none_detector.set_media_stream_classifier(Some(ArcMediaStreamClassifier::new(Arc::new(
            FailingClassifier,
        ))));
        let failed_stream = none_detector.refine_detected_mime_type(
            "video/webm",
            Some("movie.webm"),
            DetectionSource::Content(b""),
        );
        let failed_path_stream = none_detector.refine_detected_mime_type(
            "video/webm",
            Some("movie.webm"),
            DetectionSource::Path(Path::new("Cargo.toml")),
        );
        let static_classifier = StaticClassifier {
            stream_type: MediaStreamType::AudioOnly,
        };
        let classifier_trait: &dyn MediaStreamClassifier = &static_classifier;
        vec![
            classifier_present,
            selected,
            refined_by_type,
            no_source,
            no_classifier,
            disabled,
            missing_mapping,
            mismatch,
            failed_stream,
            failed_path_stream,
            detector.merge_results(&[], &[]).is_none().to_string(),
            format!("{:?}", extension_from_filename("no-extension")),
            format!(
                "{:?}",
                classifier_trait.classify_file(Path::new("Cargo.toml"))
            ),
            format!("{:?}", classifier_trait.classify_content(b"webm")),
        ]
    }

    /// Creates MIME configuration through the public config reader path.
    ///
    /// # Parameters
    /// - `enable_precise_detection`: Whether precise detection is enabled.
    /// - `precise_detection_patterns`: Extension list used for refinement.
    /// - `ambiguous_mime_mapping`: Mapping list used for refinement.
    ///
    /// # Returns
    /// Parsed MIME configuration.
    fn config_from_values(
        enable_precise_detection: bool,
        precise_detection_patterns: &str,
        ambiguous_mime_mapping: &str,
    ) -> MimeConfig {
        let mut config = Config::new();
        config
            .set(CONFIG_MIME_DETECTOR_DEFAULT, "repository")
            .expect("coverage detector selector should be configurable");
        config
            .set(CONFIG_MEDIA_STREAM_CLASSIFIER_DEFAULT, "ffprobe")
            .expect("coverage classifier selector should be configurable");
        config
            .set(
                CONFIG_MIME_ENABLE_PRECISE_DETECTION,
                enable_precise_detection,
            )
            .expect("coverage precise detection flag should be configurable");
        config
            .set(
                CONFIG_MIME_PRECISE_DETECTION_PATTERNS,
                precise_detection_patterns,
            )
            .expect("coverage precise detection patterns should be configurable");
        config
            .set(CONFIG_MIME_AMBIGUOUS_MIME_MAPPING, ambiguous_mime_mapping)
            .expect("coverage ambiguous MIME mapping should be configurable");
        MimeConfig::from_config(&config).expect("coverage MIME config should parse")
    }
}
