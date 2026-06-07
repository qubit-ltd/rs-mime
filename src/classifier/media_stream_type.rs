// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Media stream classification result.

/// Audio/video stream classification for a media payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MediaStreamType {
    /// The payload contains neither audio nor video streams.
    None,
    /// The payload contains audio streams only.
    AudioOnly,
    /// The payload contains video streams only.
    VideoOnly,
    /// The payload contains both video and audio streams.
    VideoWithAudio,
}
