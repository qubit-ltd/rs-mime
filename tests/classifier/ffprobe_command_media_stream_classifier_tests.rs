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
use qubit_config::Config;
#[cfg(unix)]
use qubit_mime::MediaStreamClassifier;
use qubit_mime::{
    CONFIG_MEDIA_STREAM_MAX_STAGING_SIZE,
    FfprobeCommandMediaStreamClassifier,
    MediaStreamType,
    MimeConfig,
};
#[cfg(unix)]
use tempfile::TempDir;

#[cfg(unix)]
use crate::support::PathEnvGuard;

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

#[test]
fn test_default_uses_disabled_logging_runner() {
    let classifier = FfprobeCommandMediaStreamClassifier::default();

    assert!(classifier.command_runner().is_logging_disabled());
}

#[test]
fn test_from_mime_config_sets_max_staging_size() {
    let mut config = Config::new();
    config
        .set(CONFIG_MEDIA_STREAM_MAX_STAGING_SIZE, 1024_u64)
        .expect("maximum staging size should be configurable");
    let classifier = FfprobeCommandMediaStreamClassifier::from_mime_config(
        MimeConfig::from_config(&config).expect("MIME config should parse"),
    );

    assert_eq!(1024, classifier.max_staging_size());
}

#[test]
#[cfg(unix)]
fn test_classify_file_uses_ffprobe_stdout_and_working_directory() {
    let temp_dir = TempDir::new().expect("temporary command directory should be created");
    let script_path = temp_dir
        .path()
        .join(FfprobeCommandMediaStreamClassifier::COMMAND);
    std::fs::write(&script_path, "#!/bin/sh\nprintf 'video\\naudio\\n'\n")
        .expect("fake ffprobe should be written");
    let mut permissions = std::fs::metadata(&script_path)
        .expect("fake ffprobe metadata should be readable")
        .permissions();
    use std::os::unix::fs::PermissionsExt;
    permissions.set_mode(0o755);
    std::fs::set_permissions(&script_path, permissions).expect("fake ffprobe should be executable");
    let _path_guard = PathEnvGuard::prepend(temp_dir.path());

    let classifier = FfprobeCommandMediaStreamClassifier::new()
        .with_command_runner(CommandRunner::new().disable_logging(true));
    let mut working_classifier = classifier.clone();
    working_classifier.set_working_directory(Some(".".to_owned()));

    assert_eq!(Some("."), working_classifier.working_directory());
    assert!(FfprobeCommandMediaStreamClassifier::is_available());
    assert_eq!(
        MediaStreamType::VideoWithAudio,
        working_classifier
            .classify_file(std::path::Path::new("Cargo.toml"))
            .expect("fake ffprobe should classify staged file")
    );
    assert_eq!(
        MediaStreamType::VideoWithAudio,
        working_classifier
            .classify_content(b"media")
            .expect("fake ffprobe should classify staged content")
    );

    let trait_classifier: &dyn MediaStreamClassifier = &working_classifier;
    assert_eq!(
        MediaStreamType::VideoWithAudio,
        trait_classifier
            .classify_file(std::path::Path::new("Cargo.toml"))
            .expect("trait object should delegate to ffprobe classifier")
    );
}

#[test]
#[cfg(unix)]
fn test_classify_file_maps_unexpected_ffprobe_exit_to_none() {
    let temp_dir = TempDir::new().expect("temporary command directory should be created");
    let script_path = temp_dir
        .path()
        .join(FfprobeCommandMediaStreamClassifier::COMMAND);
    std::fs::write(&script_path, "#!/bin/sh\nexit 7\n").expect("fake ffprobe should be written");
    let mut permissions = std::fs::metadata(&script_path)
        .expect("fake ffprobe metadata should be readable")
        .permissions();
    use std::os::unix::fs::PermissionsExt;
    permissions.set_mode(0o755);
    std::fs::set_permissions(&script_path, permissions).expect("fake ffprobe should be executable");
    let _path_guard = PathEnvGuard::prepend(temp_dir.path());

    let classifier = FfprobeCommandMediaStreamClassifier::new()
        .with_command_runner(CommandRunner::new().disable_logging(true));

    assert_eq!(
        MediaStreamType::None,
        classifier
            .classify_file(std::path::Path::new("Cargo.toml"))
            .expect("unexpected ffprobe exit should be best-effort none")
    );
}
