/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/

use std::time::Duration;

use qubit_command::CommandRunner;
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

#[test]
fn test_with_command_runner_uses_runner_configuration() {
    let runner = CommandRunner::new()
        .timeout(Duration::from_secs(2))
        .disable_logging(true);
    let mut classifier = FfprobeCommandMediaStreamClassifier::new().with_command_runner(runner);

    assert_eq!(
        Some(Duration::from_secs(2)),
        classifier.command_runner().configured_timeout()
    );
    assert!(classifier.command_runner().is_logging_disabled());

    classifier.set_command_runner(classifier.command_runner().clone().working_directory("."));
    assert_eq!(
        Some(std::path::Path::new(".")),
        classifier.command_runner().configured_working_directory()
    );
}
