// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Common MIME type constants.

/// Microsoft Excel MIME types.
pub const EXCEL_MIME_TYPES: &[&str] = &[
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    "application/vnd.ms-excel",
];

/// Default Microsoft Excel MIME type.
pub const EXCEL_MIME_TYPE: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";

/// Microsoft Word MIME types.
pub const WORD_MIME_TYPES: &[&str] = &[
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    "application/msword",
];

/// Default Microsoft Word MIME type.
pub const WORD_MIME_TYPE: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";

/// Microsoft PowerPoint MIME types.
pub const POWERPOINT_MIME_TYPES: &[&str] = &[
    "application/vnd.openxmlformats-officedocument.presentationml.presentation",
    "application/vnd.ms-powerpoint",
    "application/vnd.openxmlformats-officedocument.presentationml.slideshow",
];

/// Default Microsoft PowerPoint MIME type.
pub const POWERPOINT_MIME_TYPE: &str = "application/vnd.openxmlformats-officedocument.presentationml.presentation";

/// PDF MIME type.
pub const PDF_MIME_TYPE: &str = "application/pdf";

/// PDF MIME types.
pub const PDF_MIME_TYPES: &[&str] = &[PDF_MIME_TYPE];

/// JSON MIME type.
pub const JSON_MIME_TYPE: &str = "application/json";

/// JSON MIME types.
pub const JSON_MIME_TYPES: &[&str] = &[JSON_MIME_TYPE];

/// XML MIME type.
pub const XML_MIME_TYPE: &str = "application/xml";

/// XML MIME types.
pub const XML_MIME_TYPES: &[&str] = &[XML_MIME_TYPE];

/// CSV MIME type.
pub const CSV_MIME_TYPE: &str = "text/csv";

/// CSV MIME types.
pub const CSV_MIME_TYPES: &[&str] = &[CSV_MIME_TYPE];

/// PNG MIME type.
pub const PNG_MIME_TYPE: &str = "image/png";

/// JPEG MIME type.
pub const JPEG_MIME_TYPE: &str = "image/jpeg";

/// GIF MIME type.
pub const GIF_MIME_TYPE: &str = "image/gif";

/// MP4 MIME type.
pub const MP4_MIME_TYPE: &str = "video/mp4";

/// MP3 MIME type.
pub const MP3_MIME_TYPE: &str = "audio/mpeg";

/// WAV MIME type.
pub const WAV_MIME_TYPE: &str = "audio/wav";

/// OGG audio MIME type.
pub const OGG_MIME_TYPE: &str = "audio/ogg";

/// WEBM video MIME type.
pub const WEBM_MIME_TYPE: &str = "video/webm";

/// AVI MIME type.
pub const AVI_MIME_TYPE: &str = "video/x-msvideo";

/// FLV MIME type.
pub const FLV_MIME_TYPE: &str = "video/x-flv";

/// QuickTime MIME type.
pub const MOV_MIME_TYPE: &str = "video/quicktime";

/// WMV MIME type.
pub const WMV_MIME_TYPE: &str = "video/x-ms-wmv";
