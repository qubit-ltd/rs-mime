/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
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
