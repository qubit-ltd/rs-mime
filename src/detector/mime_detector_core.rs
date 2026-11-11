// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Shared MIME detector behavior.

use std::io::SeekFrom;
use std::sync::Arc;

use qubit_io::std_io::ReadSeek;

use super::detection_source::DetectionSource;
use crate::MediaStreamClassifier;
use crate::MediaStreamType;
use crate::MimeConfig;
use crate::MimeDetectionPolicy;
use crate::MimeError;
use crate::MimeRepository;
use crate::MimeResult;
use crate::candidate_selection::select_candidate;
use crate::repository::mime_relations::MimeRelations;

/// Shared detector core for configuration and merge/refinement logic.
///
/// # Examples
///
/// ```
/// use qubit_mime::MimeConfig;
/// use qubit_mime::MimeDetectorCore;
/// let core = MimeDetectorCore::new(MimeConfig::default());
/// assert!(core.max_buffer_size() > 0);
/// ```
#[derive(Debug, Clone)]
pub struct MimeDetectorCore {
    /// MIME detector configuration.
    config: MimeConfig,
    /// Media stream classifier.
    media_stream_classifier: Option<Arc<dyn MediaStreamClassifier>>,
    /// Repository relation context, injected by repository-backed detectors.
    mime_relations: Option<Arc<MimeRelations>>,
}

impl MimeDetectorCore {
    /// Creates a detector core from configuration.
    ///
    /// # Parameters
    /// - `config`: MIME detector configuration.
    ///
    /// # Returns
    /// Shared detector core.
    #[must_use]
    pub fn new(config: MimeConfig) -> Self {
        Self {
            config,
            media_stream_classifier: None,
            mime_relations: None,
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
    #[must_use]
    pub fn from_mime_config(config: MimeConfig) -> Self {
        Self::new(config)
    }

    /// Gets the classifier used for precise media MIME refinement.
    ///
    /// # Returns
    /// Configured classifier, or `None`.
    #[must_use]
    #[inline]
    pub fn media_stream_classifier(&self) -> Option<&dyn MediaStreamClassifier> {
        self.media_stream_classifier.as_deref()
    }

    /// Gets the maximum byte buffer size allowed for detector read paths.
    ///
    /// # Returns
    /// Maximum number of bytes this detector may allocate for one read buffer.
    #[must_use]
    #[inline]
    pub fn max_buffer_size(&self) -> usize {
        self.config.max_buffer_size()
    }

    /// Sets the classifier used for precise media MIME refinement.
    ///
    /// # Parameters
    /// - `media_stream_classifier`: Classifier to use, or `None` to disable
    ///   runtime media stream refinement.
    pub fn set_media_stream_classifier(&mut self, media_stream_classifier: Option<Arc<dyn MediaStreamClassifier>>) {
        self.media_stream_classifier = media_stream_classifier;
    }

    /// Installs the relations for the repository this core uses.
    ///
    /// # Parameters
    /// - `mime_relations`: Immutable MIME names and inheritance metadata.
    pub(crate) fn set_mime_relations(&mut self, mime_relations: Arc<MimeRelations>) {
        self.mime_relations = Some(mime_relations);
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
    pub fn merge_results(&self, from_filename: &[String], from_content: &[String]) -> Option<String> {
        self.select_candidates(from_filename, from_content, MimeDetectionPolicy::Freedesktop)
            .cloned()
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
        let result = self.select_candidates(from_filename, from_content, policy).cloned();
        let Some(result) = result else {
            return Ok(None);
        };
        if from_filename.len() == 1 && policy == MimeDetectionPolicy::PreferFilename {
            return Ok(Some(result));
        }
        self.refine_detected_mime_type(&result, filename, source).map(Some)
    }

    /// Selects candidates using the policy's equality or inheritance rule.
    ///
    /// # Parameters
    /// - `from_filename`: Candidates in filename precedence order.
    /// - `from_content`: Candidates in content precedence order.
    /// - `policy`: Rule for resolving the two candidate sources.
    ///
    /// # Returns
    /// The selected candidate borrowed from either slice, or `None`.
    fn select_candidates<'a>(
        &self,
        from_filename: &'a [String],
        from_content: &'a [String],
        policy: MimeDetectionPolicy,
    ) -> Option<&'a String> {
        let relations = (policy == MimeDetectionPolicy::Freedesktop).then(|| self.mime_relations());
        select_candidate(from_filename, from_content, policy, |filename, content| {
            relations
                .as_ref()
                .map_or(filename == content, |relations| relations.is_a(filename, content))
        })
    }

    /// Gets the configured repository relations or lazily initializes bundled
    /// relations for a standalone core's Freedesktop merge.
    fn mime_relations(&self) -> Arc<MimeRelations> {
        self.mime_relations
            .clone()
            .unwrap_or_else(|| MimeRepository::bundled_shared().relations())
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
        let Some(result) = self.select_result(from_filename, from_content, filename, policy, DetectionSource::None)?
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
    /// or not applicable. MIME names are compared without ASCII case
    /// sensitivity, while the configured output spelling is preserved. With
    /// a filename extension, the extension must be in
    /// [`MimeConfig::precise_detection_patterns`]. Without one, all configured
    /// mappings matching the detected MIME type are considered; refinement
    /// runs only when they all have the same video/audio pair. Equivalent
    /// pairs use the lexicographically smallest extension as the stable
    /// output representative. In that fallback, the pattern set is not
    /// consulted.
    ///
    /// # Errors
    /// Propagates errors returned by the configured media stream classifier.
    pub fn refine_detected_mime_type(
        &self,
        detected_mime_type: &str,
        filename: Option<&str>,
        source: DetectionSource<'_>,
    ) -> MimeResult<String> {
        let Some([video_type, audio_type]) = self.precise_detection_mapping(detected_mime_type, filename) else {
            return Ok(detected_mime_type.to_owned());
        };
        let Some(classifier) = &self.media_stream_classifier else {
            return Ok(detected_mime_type.to_owned());
        };
        let stream_type = match source {
            DetectionSource::Content(content) => classifier.classify_content(content),
            DetectionSource::Prefix(_) => return Ok(detected_mime_type.to_owned()),
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
    /// or not applicable. When the filename has no extension, mapping lookup
    /// uses all configured mappings matching the detected MIME type and
    /// proceeds only if they all use the same video/audio pair; the precise
    /// detection pattern set applies only when a filename extension exists.
    ///
    /// # Errors
    /// Propagates errors returned by the configured media stream classifier.
    /// Returns [`MimeError::Io`] when the reader position cannot be obtained,
    /// moved to the start for classification, or restored after classification.
    fn refine_detected_mime_type_from_reader(
        &self,
        detected_mime_type: &str,
        filename: Option<&str>,
        reader: &mut dyn ReadSeek,
    ) -> MimeResult<String> {
        let Some([video_type, audio_type]) = self.precise_detection_mapping(detected_mime_type, filename) else {
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
    fn precise_detection_mapping(&self, detected_mime_type: &str, filename: Option<&str>) -> Option<&[String; 2]> {
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
    fn precise_detection_mapping_key(&self, detected_mime_type: &str, filename: Option<&str>) -> Option<String> {
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
    fn precise_detection_mapping_key_by_filename(&self, detected_mime_type: &str, extension: String) -> Option<String> {
        if !self.config.precise_detection_patterns().contains(&extension) {
            return None;
        }
        let possible_mime_types = self.config.ambiguous_mime_mapping().get(&extension)?;
        if possible_mime_types
            .iter()
            .any(|mime_type| mime_type.eq_ignore_ascii_case(detected_mime_type))
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
    /// Mapping key when the MIME type is part of an unambiguous set of
    /// matching pairs. Returns `None` when matching extensions map the MIME
    /// type to different pairs, so detection preserves the original MIME type.
    fn precise_detection_mapping_key_by_mime_type(&self, detected_mime_type: &str) -> Option<String> {
        let mut selected: Option<(&String, &[String; 2])> = None;
        for (extension, pair) in self.config.ambiguous_mime_mapping() {
            if !pair.iter().any(|name| name.eq_ignore_ascii_case(detected_mime_type)) {
                continue;
            }
            if let Some((best_extension, best_pair)) = selected {
                if !mime_pair_eq_ignore_ascii_case(best_pair, pair) {
                    return None;
                }
                if extension < best_extension {
                    selected = Some((extension, pair));
                }
            } else {
                selected = Some((extension, pair));
            }
        }
        selected.map(|(extension, _)| extension.clone())
    }
}

/// Compares video/audio MIME pairs by position without ASCII case sensitivity.
///
/// # Parameters
/// - `left`: First video/audio MIME pair.
/// - `right`: Second video/audio MIME pair.
///
/// # Returns
/// `true` when both positions match without ASCII case sensitivity.
fn mime_pair_eq_ignore_ascii_case(left: &[String; 2], right: &[String; 2]) -> bool {
    left[0].eq_ignore_ascii_case(&right[0]) && left[1].eq_ignore_ascii_case(&right[1])
}

impl Default for MimeDetectorCore {
    /// Loads the default detector core.
    fn default() -> Self {
        Self::from_mime_config(MimeConfig::default())
    }
}

/// Extracts the last extension from a filename hint.
///
/// Both `/` and `\` separate path components; only the final component's
/// last dot determines the extension. This helper interprets a name hint and
/// does not access or normalize a local file path.
///
/// # Parameters
/// - `filename`: Filename hint.
///
/// # Returns
/// Lowercase extension without a leading dot.
fn extension_from_filename(filename: &str) -> Option<String> {
    filename
        .rsplit(['/', '\\'])
        .next()
        .and_then(|basename| basename.rsplit_once('.').map(|(_, extension)| extension))
        .filter(|extension| !extension.is_empty())
        .map(str::to_ascii_lowercase)
}
