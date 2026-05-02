/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/

use qubit_mime::{FfprobeCommandMediaStreamClassifier, MediaStreamType};

#[test]
fn test_classify_stream_listing_maps_ffprobe_output() {
    assert_eq!(
        MediaStreamType::VideoWithAudio,
        FfprobeCommandMediaStreamClassifier::classify_stream_listing("video\naudio\n")
    );
    assert_eq!(
        MediaStreamType::VideoOnly,
        FfprobeCommandMediaStreamClassifier::classify_stream_listing("video\n")
    );
    assert_eq!(
        MediaStreamType::AudioOnly,
        FfprobeCommandMediaStreamClassifier::classify_stream_listing("audio\n")
    );
    assert_eq!(
        MediaStreamType::None,
        FfprobeCommandMediaStreamClassifier::classify_stream_listing("data\n")
    );
}
