// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Unit tests for MIME refinement through native basenames.

use std::io::Read;
use std::path::Path;
use std::sync::Arc;

use qubit_config::Config;

use crate::CONFIG_MEDIA_STREAM_CLASSIFIER_DEFAULT;
use crate::CONFIG_MIME_AMBIGUOUS_MIME_MAPPING;
use crate::CONFIG_MIME_DETECTOR_DEFAULT;
use crate::CONFIG_MIME_ENABLE_PRECISE_DETECTION;
use crate::CONFIG_MIME_PRECISE_DETECTION_PATTERNS;
use crate::DetectionSource;
use crate::MediaStreamType;
use crate::MimeConfig;
use crate::MimeDetectionPolicy;
use crate::MimeResult;
use crate::classifier::media_stream_classifier::MediaStreamClassifier;
use crate::detector::mime_detector_core::MimeDetectorCore;

#[derive(Debug)]
struct AudioOnlyClassifier;

impl MediaStreamClassifier for AudioOnlyClassifier {
    fn classify_file(&self, _file: &Path) -> MimeResult<MediaStreamType> {
        Ok(MediaStreamType::AudioOnly)
    }

    fn classify_reader(&self, _reader: &mut dyn Read) -> MimeResult<MediaStreamType> {
        Ok(MediaStreamType::AudioOnly)
    }
}

#[test]
fn test_extensionless_basename_uses_unique_mime_mapping() {
    let mut raw = Config::new();
    raw.set(CONFIG_MIME_DETECTOR_DEFAULT, "repository")
        .expect("detector selection should be valid");
    raw.set(CONFIG_MEDIA_STREAM_CLASSIFIER_DEFAULT, "ffprobe")
        .expect("classifier selection should be valid");
    raw.set(CONFIG_MIME_ENABLE_PRECISE_DETECTION, true)
        .expect("precise detection should be configurable");
    raw.set(CONFIG_MIME_PRECISE_DETECTION_PATTERNS, "ogg")
        .expect("precise detection patterns should be configurable");
    raw.set(CONFIG_MIME_AMBIGUOUS_MIME_MAPPING, "ogg:video/ogg,audio/ogg")
        .expect("ambiguous mappings should be configurable");
    let config = MimeConfig::from_config(&raw).expect("MIME configuration should parse");
    let mut core = MimeDetectorCore::new(config);
    core.set_media_stream_classifier(Some(Arc::new(AudioOnlyClassifier)));

    assert_eq!(
        Some("audio/ogg".to_owned()),
        core.select_result_for_basename(
            &[],
            &["video/ogg".to_owned()],
            Some("movie"),
            MimeDetectionPolicy::VerifyContent,
            DetectionSource::Path(Path::new("movie")),
        )
        .expect("extensionless path refinement should succeed")
    );
}
