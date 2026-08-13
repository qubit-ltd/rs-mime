// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Shared MIME detector behavior.

use std::io::SeekFrom;
use std::path::Path;
use std::sync::Arc;

use qubit_io::std_io::ReadSeek;

use crate::{
    MediaStreamClassifier, MediaStreamType, MimeConfig, MimeDetectionPolicy, MimeError, MimeResult,
};

use super::detection_source::DetectionSource;

/// Shared detector core for configuration and merge/refinement logic.
#[derive(Debug, Clone)]
pub struct MimeDetectorCore {
    /// MIME detector configuration.
    config: MimeConfig,
    /// Media stream classifier.
    media_stream_classifier: Option<Arc<dyn MediaStreamClassifier>>,
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

    /// Creates a detector core from configuration without implicit classifiers.
    ///
    /// # Parameters
    /// - `config`: MIME detector configuration.
    ///
    /// # Returns
    /// Shared detector core. Applications that need precise media refinement
    /// inject a classifier explicitly with
    /// [`Self::set_media_stream_classifier`].
    pub fn from_mime_config(config: MimeConfig) -> Self {
        Self::new(config)
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
    pub fn media_stream_classifier(&self) -> Option<&dyn MediaStreamClassifier> {
        self.media_stream_classifier.as_deref()
    }

    /// Gets the maximum byte buffer size allowed for detector read paths.
    ///
    /// # Returns
    /// Maximum number of bytes this detector may allocate for one read buffer.
    pub fn max_buffer_size(&self) -> usize {
        self.config.max_buffer_size()
    }

    /// Merges filename and content candidates using the detector selection
    /// strategy.
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
    ) -> MimeResult<Option<String>> {
        let result = if from_filename.len() == 1 && policy == MimeDetectionPolicy::PreferFilename {
            from_filename.first().cloned()
        } else {
            self.merge_results(from_filename, from_content)
        };
        let Some(result) = result else {
            return Ok(None);
        };
        self.refine_detected_mime_type(&result, filename, source)
            .map(Some)
    }

    /// Selects and refines a MIME type using a seekable reader.
    ///
    /// The reader is classified only when content verification is required
    /// and precise media refinement applies. Classification starts at byte
    /// zero, and the reader's original position is restored before this method
    /// returns.
    ///
    /// # Parameters
    /// - `from_filename`: Candidates from filename glob detection.
    /// - `from_content`: Candidates from content detection.
    /// - `filename`: Optional filename used for precise detection decisions.
    /// - `policy`: Strategy for resolving filename and content results.
    /// - `reader`: Seekable source available for media stream refinement.
    ///
    /// # Returns
    /// Selected and optionally refined MIME type name, or `None` when no
    /// candidate is available.
    ///
    /// # Errors
    /// Returns [`MimeError::Io`] when the reader position cannot be obtained or
    /// restored, and propagates media classifier errors.
    pub fn select_reader_result(
        &self,
        from_filename: &[String],
        from_content: &[String],
        filename: Option<&str>,
        policy: MimeDetectionPolicy,
        reader: &mut dyn ReadSeek,
    ) -> MimeResult<Option<String>> {
        let Some(result) = self.select_result(
            from_filename,
            from_content,
            filename,
            policy,
            DetectionSource::None,
        )?
        else {
            return Ok(None);
        };
        if from_filename.len() == 1 && policy == MimeDetectionPolicy::PreferFilename {
            return Ok(Some(result));
        }
        self.refine_detected_mime_type_from_reader(&result, filename, reader)
            .map(Some)
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
    /// or not applicable.
    ///
    /// # Errors
    /// Propagates errors returned by the configured media stream classifier.
    pub fn refine_detected_mime_type(
        &self,
        detected_mime_type: &str,
        filename: Option<&str>,
        source: DetectionSource<'_>,
    ) -> MimeResult<String> {
        let Some([video_type, audio_type]) =
            self.precise_detection_mapping(detected_mime_type, filename)
        else {
            return Ok(detected_mime_type.to_owned());
        };
        let Some(classifier) = &self.media_stream_classifier else {
            return Ok(detected_mime_type.to_owned());
        };
        let stream_type = match source {
            DetectionSource::Content(content) => classifier.classify_content(content),
            DetectionSource::Path(path) => classifier.classify_file(path),
            DetectionSource::None => return Ok(detected_mime_type.to_owned()),
        };
        match stream_type? {
            MediaStreamType::AudioOnly => Ok(audio_type.clone()),
            MediaStreamType::VideoOnly | MediaStreamType::VideoWithAudio => Ok(video_type.clone()),
            MediaStreamType::None => Ok(detected_mime_type.to_owned()),
        }
    }

    /// Refines an ambiguous media MIME type using a seekable reader.
    ///
    /// # Parameters
    /// - `detected_mime_type`: Initial MIME type name.
    /// - `filename`: Optional filename used to resolve the ambiguous mapping.
    /// - `reader`: Seekable stream to classify without consuming its position.
    ///
    /// # Returns
    /// Refined MIME type name, or the original type if refinement is disabled
    /// or the classifier fails.
    ///
    /// # Errors
    /// Returns [`MimeError::Io`] when the reader position cannot be obtained or
    /// restored.
    fn refine_detected_mime_type_from_reader(
        &self,
        detected_mime_type: &str,
        filename: Option<&str>,
        reader: &mut dyn ReadSeek,
    ) -> MimeResult<String> {
        let Some([video_type, audio_type]) =
            self.precise_detection_mapping(detected_mime_type, filename)
        else {
            return Ok(detected_mime_type.to_owned());
        };
        let Some(classifier) = &self.media_stream_classifier else {
            return Ok(detected_mime_type.to_owned());
        };
        let original_position = reader.stream_position()?;
        if let Err(error) = reader.seek(SeekFrom::Start(0)) {
            return match reader.seek(SeekFrom::Start(original_position)) {
                Ok(_) => Err(MimeError::Io(error)),
                Err(restore_error) => Err(MimeError::Io(restore_error)),
            };
        }
        let stream_type = classifier.classify_reader(reader);
        let restore_result = reader.seek(SeekFrom::Start(original_position));
        let stream_type = match (stream_type, restore_result) {
            (_, Err(error)) => return Err(MimeError::Io(error)),
            (Ok(stream_type), Ok(_)) => stream_type,
            (Err(error), Ok(_)) => return Err(error),
        };
        Ok(match stream_type {
            MediaStreamType::AudioOnly => audio_type.clone(),
            MediaStreamType::VideoOnly | MediaStreamType::VideoWithAudio => video_type.clone(),
            MediaStreamType::None => detected_mime_type.to_owned(),
        })
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
