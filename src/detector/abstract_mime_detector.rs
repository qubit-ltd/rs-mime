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
use std::sync::Arc;

use crate::{
    ArcMediaStreamClassifier, MediaStreamClassifier, MediaStreamType, MimeConfig,
    MimeDetectionPolicy,
};

use super::detection_source::DetectionSource;

/// Java-style shared detector state and merge/refinement logic.
#[derive(Debug, Clone)]
pub struct AbstractMimeDetector {
    /// MIME detector configuration.
    config: MimeConfig,
    /// Media stream classifier.
    media_stream_classifier: Option<Arc<dyn MediaStreamClassifier>>,
}

impl AbstractMimeDetector {
    /// Creates detector state from configuration.
    ///
    /// # Parameters
    /// - `config`: MIME detector configuration.
    ///
    /// # Returns
    /// Shared detector state.
    pub fn new(config: MimeConfig) -> Self {
        Self {
            config,
            media_stream_classifier: None,
        }
    }

    /// Creates detector state from configuration and default classifier.
    ///
    /// # Parameters
    /// - `config`: MIME detector configuration.
    ///
    /// # Returns
    /// Shared detector state using the configured default media classifier when
    /// precise detection is enabled.
    pub fn from_mime_config(config: MimeConfig) -> Self {
        let mut detector = Self::new(config.clone());
        if config.enable_precise_detection() {
            detector.set_media_stream_classifier(Some(
                ArcMediaStreamClassifier::from_mime_config(&config).into_inner(),
            ));
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
        media_stream_classifier: Option<Arc<dyn MediaStreamClassifier>>,
    ) {
        self.media_stream_classifier = media_stream_classifier;
    }

    /// Gets the classifier used for precise media MIME refinement.
    ///
    /// # Returns
    /// Configured classifier, or `None`.
    pub fn media_stream_classifier(&self) -> Option<&Arc<dyn MediaStreamClassifier>> {
        self.media_stream_classifier.as_ref()
    }

    /// Merges filename and content candidates using the Java detector strategy.
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
        let result = if from_filename.len() == 1 && !policy.should_verify_content() {
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

impl Default for AbstractMimeDetector {
    /// Loads default detector state.
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

#[cfg(coverage)]
pub(crate) mod coverage_support {
    //! Coverage helpers for shared detector behavior.

    use std::path::Path;
    use std::sync::Arc;

    use crate::{
        MediaStreamClassifier, MediaStreamType, MimeConfig, MimeError,
        StringListMimeDetectorBackend,
    };

    use super::{AbstractMimeDetector, DetectionSource, extension_from_filename};

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
        fn classify_content(&self, _content: &[u8]) -> crate::MimeResult<MediaStreamType> {
            Ok(self.stream_type)
        }
    }

    struct StaticBackend;

    impl StringListMimeDetectorBackend for StaticBackend {
        /// Returns a static filename candidate list.
        fn guess_from_filename(&self, _filename: &str) -> Vec<String> {
            vec!["video/webm".to_owned()]
        }

        /// Returns a static content candidate list.
        fn guess_from_content(&self, _content: &[u8]) -> Vec<String> {
            vec!["audio/webm".to_owned()]
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
        fn classify_content(&self, _content: &[u8]) -> crate::MimeResult<MediaStreamType> {
            Err(MimeError::invalid_classifier_input("forced"))
        }
    }

    /// Exercises shared merge, precise detection, and helper branches.
    ///
    /// # Returns
    /// Summary strings from shared detector behavior.
    pub(crate) fn exercise_abstract_edges() -> Vec<String> {
        let mut detector = AbstractMimeDetector::new(MimeConfig::new(
            "repository",
            "ffprobe",
            true,
            "webm,ogg",
            "webm:video/webm,audio/webm;ogg:video/ogg,audio/ogg",
        ));
        detector.set_media_stream_classifier(Some(Arc::new(StaticClassifier {
            stream_type: MediaStreamType::VideoOnly,
        })));
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
        let no_classifier = AbstractMimeDetector::new(MimeConfig::new(
            "repository",
            "ffprobe",
            true,
            "webm",
            "webm:video/webm,audio/webm",
        ))
        .refine_detected_mime_type(
            "video/webm",
            Some("movie.webm"),
            DetectionSource::Content(b""),
        );
        let disabled = AbstractMimeDetector::new(MimeConfig::new(
            "repository",
            "ffprobe",
            false,
            "webm",
            "webm:video/webm,audio/webm",
        ))
        .refine_detected_mime_type(
            "video/webm",
            Some("movie.webm"),
            DetectionSource::Content(b""),
        );
        let missing_mapping =
            AbstractMimeDetector::new(MimeConfig::new("repository", "ffprobe", true, "webm", ""))
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
        let mut none_detector = AbstractMimeDetector::new(MimeConfig::new(
            "repository",
            "ffprobe",
            true,
            "webm",
            "webm:video/webm,audio/webm",
        ));
        none_detector.set_media_stream_classifier(Some(Arc::new(FailingClassifier)));
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
        let backend = StaticBackend;
        let backend_trait: &dyn StringListMimeDetectorBackend = &backend;
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
            backend_trait.guess_from_filename("movie.webm").join(","),
            backend_trait.guess_from_content(b"webm").join(","),
            format!(
                "{:?}",
                classifier_trait.classify_file(Path::new("Cargo.toml"))
            ),
            format!("{:?}", classifier_trait.classify_content(b"webm")),
        ]
    }
}
