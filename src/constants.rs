// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Configuration constants used by MIME detectors.

use std::time::Duration;

/// Application-level collision budget for MIME staging names.
pub(crate) const DEFAULT_TEMP_NAME_MAX_ATTEMPTS: usize = 256;

/// Environment variable selecting the default MIME detector implementation.
pub const ENV_MIME_DETECTOR_DEFAULT: &str = "QUBIT_MIME_DETECTOR_DEFAULT";

/// Environment variable listing fallback MIME detector implementations.
pub const ENV_MIME_DETECTOR_FALLBACKS: &str = "QUBIT_MIME_DETECTOR_FALLBACKS";

/// Environment variable selecting the default media stream classifier
/// implementation.
pub const ENV_MEDIA_STREAM_CLASSIFIER_DEFAULT: &str = "QUBIT_MEDIA_STREAM_CLASSIFIER_DEFAULT";

/// Environment variable limiting temporary staging for media stream classifier
/// input.
pub const ENV_MEDIA_STREAM_MAX_STAGING_SIZE: &str = "QUBIT_MEDIA_STREAM_MAX_STAGING_SIZE";

/// Environment variable limiting retained stdout and stderr bytes for native
/// command-based MIME detection.
pub const ENV_COMMAND_OUTPUT_MAX_BYTES: &str = "QUBIT_MIME_COMMAND_OUTPUT_MAX_BYTES";

/// Environment variable controlling command timeout for native MIME detectors.
pub const ENV_COMMAND_TIMEOUT: &str = "QUBIT_MIME_COMMAND_TIMEOUT";

/// Environment variable controlling precise MIME detection.
pub const ENV_MIME_DETECTOR_ENABLE_PRECISE_DETECTION: &str = "QUBIT_MIME_ENABLE_PRECISE_DETECTION";

/// Environment variable listing extensions that should use precise detection.
pub const ENV_MIME_DETECTOR_PRECISE_DETECTION_PATTERNS: &str = "QUBIT_MIME_PRECISE_DETECTION_PATTERNS";

/// Environment variable defining ambiguous extension to MIME mappings.
pub const ENV_MIME_DETECTOR_AMBIGUOUS_MIME_MAPPING: &str = "QUBIT_MIME_AMBIGUOUS_MIME_MAPPING";

/// Environment variable limiting detector buffer allocations.
pub const ENV_MIME_MAX_BUFFER_SIZE: &str = "QUBIT_MIME_MAX_BUFFER_SIZE";

/// Configuration key selecting the default MIME detector implementation.
pub const CONFIG_MIME_DETECTOR_DEFAULT: &str = "mime.detector.default";

/// Configuration key listing fallback MIME detector implementations.
pub const CONFIG_MIME_DETECTOR_FALLBACKS: &str = "mime.detector.fallbacks";

/// Configuration key selecting the default media stream classifier
/// implementation.
pub const CONFIG_MEDIA_STREAM_CLASSIFIER_DEFAULT: &str = "mime.media.stream.classifier.default";

/// Configuration key limiting temporary staging for media stream classifier
/// input.
pub const CONFIG_MEDIA_STREAM_MAX_STAGING_SIZE: &str = "mime.media.stream.max.staging.size";

/// Configuration key limiting retained stdout and stderr bytes for native
/// command-based MIME detection.
pub const CONFIG_COMMAND_OUTPUT_MAX_BYTES: &str = "mime.command.output.max.bytes";

/// Configuration key controlling command timeout for native MIME detectors.
pub const CONFIG_COMMAND_TIMEOUT: &str = "mime.command.timeout";

/// Configuration key controlling precise MIME detection.
pub const CONFIG_MIME_ENABLE_PRECISE_DETECTION: &str = "mime.enable.precise.detection";

/// Configuration key listing extensions that should use precise detection.
pub const CONFIG_MIME_PRECISE_DETECTION_PATTERNS: &str = "mime.precise.detection.patterns";

/// Configuration key defining ambiguous extension to MIME mappings.
pub const CONFIG_MIME_AMBIGUOUS_MIME_MAPPING: &str = "mime.ambiguous.mime.mapping";

/// Configuration key limiting detector buffer allocations.
pub const CONFIG_MIME_MAX_BUFFER_SIZE: &str = "mime.max.buffer.size";

/// Default MIME detector backend selector.
pub const DEFAULT_MIME_DETECTOR: &str = "repository";

/// Default fallback MIME detector backend selector list.
pub const DEFAULT_MIME_DETECTOR_FALLBACKS: &str = "";

/// Default media stream classifier backend selector.
pub const DEFAULT_MEDIA_STREAM_CLASSIFIER: &str = "ffprobe";

/// Default maximum bytes staged from reader/content input for media stream
/// classification.
pub const DEFAULT_MEDIA_STREAM_MAX_STAGING_SIZE: u64 = 64 * 1024 * 1024;

/// Default retained stdout and stderr byte limit for each native command-based
/// MIME detection stream.
pub const DEFAULT_COMMAND_OUTPUT_MAX_BYTES: usize = 64 * 1024;

/// Default command timeout for native MIME detector commands.
pub const DEFAULT_COMMAND_TIMEOUT: Duration = Duration::from_secs(30);

/// Default value for precise media stream based detection.
pub const DEFAULT_ENABLE_PRECISE_DETECTION: bool = true;

/// Default comma-separated extensions that may need media stream
/// classification.
pub const DEFAULT_PRECISE_DETECTION_PATTERNS: &str = "webm,ogg";

/// Default ambiguous extension mapping: `extension:video_mime,audio_mime`.
pub const DEFAULT_AMBIGUOUS_MIME_MAPPING: &str = "webm:video/webm,audio/webm;ogg:video/ogg,audio/ogg";

/// Default maximum byte buffer size used by detector read paths.
pub const DEFAULT_MIME_MAX_BUFFER_SIZE: usize = 16 * 1024 * 1024;
