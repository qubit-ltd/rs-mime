/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Configuration constants used by MIME detectors.
//!

/// Environment variable selecting the default MIME detector implementation.
pub const ENV_MIME_DETECTOR_DEFAULT: &str = "QUBIT_MIME_DETECTOR_DEFAULT";

/// Environment variable listing fallback MIME detector implementations.
pub const ENV_MIME_DETECTOR_FALLBACKS: &str = "QUBIT_MIME_DETECTOR_FALLBACKS";

/// Environment variable selecting the default media stream classifier implementation.
pub const ENV_MEDIA_STREAM_CLASSIFIER_DEFAULT: &str = "QUBIT_MEDIA_STREAM_CLASSIFIER_DEFAULT";

/// Environment variable controlling precise MIME detection.
pub const ENV_MIME_DETECTOR_ENABLE_PRECISE_DETECTION: &str = "QUBIT_MIME_ENABLE_PRECISE_DETECTION";

/// Environment variable listing extensions that should use precise detection.
pub const ENV_MIME_DETECTOR_PRECISE_DETECTION_PATTERNS: &str =
    "QUBIT_MIME_PRECISE_DETECTION_PATTERNS";

/// Environment variable defining ambiguous extension to MIME mappings.
pub const ENV_MIME_DETECTOR_AMBIGUOUS_MIME_MAPPING: &str = "QUBIT_MIME_AMBIGUOUS_MIME_MAPPING";

/// Configuration key selecting the default MIME detector implementation.
pub const CONFIG_MIME_DETECTOR_DEFAULT: &str = "mime.detector.default";

/// Configuration key listing fallback MIME detector implementations.
pub const CONFIG_MIME_DETECTOR_FALLBACKS: &str = "mime.detector.fallbacks";

/// Configuration key selecting the default media stream classifier implementation.
pub const CONFIG_MEDIA_STREAM_CLASSIFIER_DEFAULT: &str = "mime.media.stream.classifier.default";

/// Configuration key controlling precise MIME detection.
pub const CONFIG_MIME_ENABLE_PRECISE_DETECTION: &str = "mime.enable.precise.detection";

/// Configuration key listing extensions that should use precise detection.
pub const CONFIG_MIME_PRECISE_DETECTION_PATTERNS: &str = "mime.precise.detection.patterns";

/// Configuration key defining ambiguous extension to MIME mappings.
pub const CONFIG_MIME_AMBIGUOUS_MIME_MAPPING: &str = "mime.ambiguous.mime.mapping";

/// Default MIME detector backend selector.
pub const DEFAULT_MIME_DETECTOR: &str = "repository";

/// Default fallback MIME detector backend selector list.
pub const DEFAULT_MIME_DETECTOR_FALLBACKS: &str = "";

/// Default media stream classifier backend selector.
pub const DEFAULT_MEDIA_STREAM_CLASSIFIER: &str = "ffprobe";

/// Default value for precise media stream based detection.
pub const DEFAULT_ENABLE_PRECISE_DETECTION: bool = true;

/// Default comma-separated extensions that may need media stream classification.
pub const DEFAULT_PRECISE_DETECTION_PATTERNS: &str = "webm,ogg";

/// Default ambiguous extension mapping: `extension:video_mime,audio_mime`.
pub const DEFAULT_AMBIGUOUS_MIME_MAPPING: &str =
    "webm:video/webm,audio/webm;ogg:video/ogg,audio/ogg";
