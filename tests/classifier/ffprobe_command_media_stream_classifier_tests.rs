// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::time::Duration;

use qubit_command::CommandRunner;
use qubit_config::Config;
#[cfg(unix)]
use qubit_local_files::{
    LocalFileSystem,
    LocalTempDirectoryOptions,
};
#[cfg(unix)]
use qubit_mime::MediaStreamClassifier;
use qubit_mime::{
    CONFIG_COMMAND_OUTPUT_MAX_BYTES,
    CONFIG_COMMAND_TIMEOUT,
    CONFIG_MEDIA_STREAM_MAX_STAGING_SIZE,
    DEFAULT_COMMAND_OUTPUT_MAX_BYTES,
    DEFAULT_COMMAND_TIMEOUT,
    FfprobeCommandMediaStreamClassifier,
    MediaStreamType,
    MimeConfig,
};

#[cfg(unix)]
use crate::support::PathEnvGuard;

#[test]
fn test_classify_stream_listing_maps_ffprobe_output() {
    assert_eq!(
        MediaStreamType::VideoWithAudio,
        FfprobeCommandMediaStreamClassifier::classify_stream_listing(
            "video\naudio\n"
        )
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
    let runner =
        CommandRunner::new(Duration::from_secs(2)).disable_logging(true);
    let mut classifier =
        FfprobeCommandMediaStreamClassifier::new().with_command_runner(runner);

    assert_eq!(
        Some(Duration::from_secs(2)),
        classifier.command_runner().configured_timeout()
    );
    assert!(classifier.command_runner().is_logging_disabled());

    classifier.set_command_runner(
        classifier.command_runner().clone().working_directory("."),
    );
    assert_eq!(
        Some(std::path::Path::new(".")),
        classifier.command_runner().configured_working_directory()
    );
}

#[test]
fn test_default_uses_disabled_logging_runner() {
    let classifier = FfprobeCommandMediaStreamClassifier::default();

    assert!(classifier.command_runner().is_logging_disabled());
    assert_eq!(
        classifier.command_runner().configured_timeout(),
        Some(DEFAULT_COMMAND_TIMEOUT),
    );
    assert_eq!(
        classifier.command_runner().configured_max_stdout_bytes(),
        Some(DEFAULT_COMMAND_OUTPUT_MAX_BYTES),
    );
    assert_eq!(
        classifier.command_runner().configured_max_stderr_bytes(),
        Some(DEFAULT_COMMAND_OUTPUT_MAX_BYTES),
    );
    assert!(
        classifier
            .command_runner()
            .is_output_truncation_failure_enabled()
    );
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
fn test_from_mime_config_limits_ffprobe_output() {
    let mut config = Config::new();
    config
        .set(CONFIG_COMMAND_TIMEOUT, "31s")
        .expect("command timeout should be configurable");
    config
        .set(CONFIG_COMMAND_OUTPUT_MAX_BYTES, 1024_u64)
        .expect("command output limit should be configurable");
    let classifier = FfprobeCommandMediaStreamClassifier::from_mime_config(
        MimeConfig::from_config(&config).expect("MIME config should parse"),
    );

    assert_eq!(
        Some(Duration::from_secs(31)),
        classifier.command_runner().configured_timeout()
    );
    assert_eq!(
        Some(1024),
        classifier.command_runner().configured_max_stdout_bytes()
    );
    assert_eq!(
        Some(1024),
        classifier.command_runner().configured_max_stderr_bytes()
    );
    assert!(
        classifier
            .command_runner()
            .is_output_truncation_failure_enabled()
    );
}

#[test]
fn test_from_mime_config_uses_configured_timeout() {
    let mut config = Config::new();
    config
        .set(CONFIG_COMMAND_TIMEOUT, "2500ms")
        .expect("command timeout should be configurable");
    let classifier = FfprobeCommandMediaStreamClassifier::from_mime_config(
        MimeConfig::from_config(&config).expect("MIME config should parse"),
    );

    assert_eq!(
        Some(Duration::from_millis(2500)),
        classifier.command_runner().configured_timeout()
    );
}

#[test]
fn test_from_mime_config_limits_ffprobe_output_and_stage_size() {
    let mut config = Config::new();
    config
        .set(CONFIG_COMMAND_OUTPUT_MAX_BYTES, 1024_u64)
        .expect("command output limit should be configurable");
    let classifier = FfprobeCommandMediaStreamClassifier::from_mime_config(
        MimeConfig::from_config(&config).expect("MIME config should parse"),
    );

    assert_eq!(
        Some(1024),
        classifier.command_runner().configured_max_stdout_bytes()
    );
    assert_eq!(
        Some(1024),
        classifier.command_runner().configured_max_stderr_bytes()
    );
    assert!(
        classifier
            .command_runner()
            .is_output_truncation_failure_enabled()
    );
}

#[test]
fn test_max_staging_size_accessors_update_limit() {
    let mut classifier = FfprobeCommandMediaStreamClassifier::new();

    classifier.set_max_staging_size(2048);
    assert_eq!(2048, classifier.max_staging_size());

    let classifier = classifier.with_max_staging_size(4096);
    assert_eq!(4096, classifier.max_staging_size());
}

#[test]
#[cfg(unix)]
fn test_classify_file_uses_ffprobe_stdout_and_working_directory() {
    let temp_dir = LocalFileSystem::host()
        .create_temp_directory(&LocalTempDirectoryOptions::new())
        .expect("temporary command directory should be created");
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
    std::fs::set_permissions(&script_path, permissions)
        .expect("fake ffprobe should be executable");
    let _path_guard = PathEnvGuard::prepend(temp_dir.path());

    let classifier = FfprobeCommandMediaStreamClassifier::new()
        .with_command_runner(
            CommandRunner::new(DEFAULT_COMMAND_TIMEOUT).disable_logging(true),
        );
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
fn test_classify_file_propagates_ffprobe_start_error() {
    let temp_dir = LocalFileSystem::host()
        .create_temp_directory(&LocalTempDirectoryOptions::new())
        .expect("temporary command directory should be created");
    let _path_guard = PathEnvGuard::set(temp_dir.path());
    let classifier = FfprobeCommandMediaStreamClassifier::new()
        .with_command_runner(
            CommandRunner::new(DEFAULT_COMMAND_TIMEOUT).disable_logging(true),
        );

    let private_path =
        temp_dir.path().join("private-customer-source-video.mp4");
    std::fs::write(&private_path, b"media")
        .expect("private media fixture should be written");
    let error = classifier
        .classify_file(&private_path)
        .expect_err("missing ffprobe executable should report command error");

    assert!(error.to_string().contains("ffprobe"));
    assert!(
        !error
            .to_string()
            .contains(private_path.to_string_lossy().as_ref())
    );
    assert!(
        !format!("{error:?}").contains(private_path.to_string_lossy().as_ref())
    );
}

#[test]
#[cfg(unix)]
fn test_classify_file_maps_unexpected_ffprobe_exit_to_none() {
    let temp_dir = LocalFileSystem::host()
        .create_temp_directory(&LocalTempDirectoryOptions::new())
        .expect("temporary command directory should be created");
    let script_path = temp_dir
        .path()
        .join(FfprobeCommandMediaStreamClassifier::COMMAND);
    std::fs::write(&script_path, "#!/bin/sh\nexit 7\n")
        .expect("fake ffprobe should be written");
    let mut permissions = std::fs::metadata(&script_path)
        .expect("fake ffprobe metadata should be readable")
        .permissions();
    use std::os::unix::fs::PermissionsExt;
    permissions.set_mode(0o755);
    std::fs::set_permissions(&script_path, permissions)
        .expect("fake ffprobe should be executable");
    let _path_guard = PathEnvGuard::prepend(temp_dir.path());

    let classifier = FfprobeCommandMediaStreamClassifier::new()
        .with_command_runner(
            CommandRunner::new(DEFAULT_COMMAND_TIMEOUT).disable_logging(true),
        );

    assert_eq!(
        MediaStreamType::None,
        classifier
            .classify_file(std::path::Path::new("Cargo.toml"))
            .expect("unexpected ffprobe exit should be best-effort none")
    );
}
