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
use qubit_mime::{
    CONFIG_MEDIA_STREAM_CLASSIFIER_DEFAULT,
    CONFIG_MIME_DETECTOR_DEFAULT,
    CONFIG_MIME_ENABLE_PRECISE_DETECTION,
    FileBasedMimeDetector,
    FileCommandMimeDetector,
    MimeConfig,
    MimeDetectionPolicy,
    MimeDetector,
    MimeRepository,
};
use tempfile::TempDir;

use crate::support::PathEnvGuard;

#[test]
fn test_detect_by_filename_uses_repository_candidates() {
    let detector = FileCommandMimeDetector::new();

    assert_eq!(
        Some("image/jpeg".to_owned()),
        detector.detect_by_filename("photo.jpg")
    );
}

#[test]
fn test_is_available_can_be_called_without_panicking() {
    let _ = FileCommandMimeDetector::is_available();
}

#[test]
fn test_with_repository_and_runner_uses_runner_configuration() {
    let repository = MimeRepository::empty();
    let runner = CommandRunner::new()
        .timeout(Duration::from_secs(2))
        .disable_logging(true);
    let mut detector = FileCommandMimeDetector::with_repository_and_runner(&repository, runner);

    assert_eq!(
        Some(Duration::from_secs(2)),
        detector.command_runner().configured_timeout()
    );
    assert!(detector.command_runner().is_logging_disabled());

    detector.set_command_runner(detector.command_runner().clone().working_directory("."));
    assert_eq!(
        Some(std::path::Path::new(".")),
        detector.command_runner().configured_working_directory()
    );

    detector.set_command_runner(CommandRunner::new().disable_logging(false));
    assert!(!detector.command_runner().is_logging_disabled());
}

#[test]
fn test_detect_file_by_content_uses_runner_timeout() {
    if !FileCommandMimeDetector::is_available() {
        return;
    }
    let detector = FileCommandMimeDetector::new()
        .with_command_runner(CommandRunner::new().timeout(Duration::ZERO));

    assert!(
        detector
            .detect_file_by_content(std::path::Path::new("Cargo.toml"))
            .is_err()
    );
}

#[test]
#[cfg(unix)]
fn test_detect_file_by_content_reads_file_command_stdout() {
    let temp_dir = TempDir::new().expect("temporary command directory should be created");
    let script_path = temp_dir.path().join(FileCommandMimeDetector::COMMAND);
    std::fs::write(&script_path, "#!/bin/sh\nprintf 'text/plain\\n'\n")
        .expect("fake file command should be written");
    let mut permissions = std::fs::metadata(&script_path)
        .expect("fake file command metadata should be readable")
        .permissions();
    use std::os::unix::fs::PermissionsExt;
    permissions.set_mode(0o755);
    std::fs::set_permissions(&script_path, permissions)
        .expect("fake file command should be executable");
    let _path_guard = PathEnvGuard::prepend(temp_dir.path());
    let repository = MimeRepository::empty();
    let detector = FileCommandMimeDetector::with_repository_and_runner(
        &repository,
        CommandRunner::new().disable_logging(true),
    );

    assert_eq!(0, detector.repository().all().len());
    assert_eq!(
        Some("text/plain".to_owned()),
        detector
            .detect_file_by_content(std::path::Path::new("Cargo.toml"))
            .expect("fake file command should return MIME text")
    );
    assert_eq!(
        Some("text/plain".to_owned()),
        detector.detect_by_content(b"plain text")
    );
    let mut reader = std::io::Cursor::new(b"plain text".to_vec());
    assert_eq!(
        Some("text/plain".to_owned()),
        detector
            .detect_reader(&mut reader, None, MimeDetectionPolicy::VerifyContent)
            .expect("fake file command should support reader detection")
    );
    assert_eq!(
        Some("text/plain".to_owned()),
        detector
            .detect_file(
                std::path::Path::new("Cargo.toml"),
                MimeDetectionPolicy::PreferFilename,
            )
            .expect("fake file command should support full file detection")
    );
}

#[test]
#[cfg(unix)]
fn test_detect_file_by_content_returns_none_for_empty_stdout() {
    let temp_dir = TempDir::new().expect("temporary command directory should be created");
    let script_path = temp_dir.path().join(FileCommandMimeDetector::COMMAND);
    std::fs::write(&script_path, "#!/bin/sh\nexit 0\n")
        .expect("fake file command should be written");
    let mut permissions = std::fs::metadata(&script_path)
        .expect("fake file command metadata should be readable")
        .permissions();
    use std::os::unix::fs::PermissionsExt;
    permissions.set_mode(0o755);
    std::fs::set_permissions(&script_path, permissions)
        .expect("fake file command should be executable");
    let _path_guard = PathEnvGuard::prepend(temp_dir.path());
    let repository = MimeRepository::empty();
    let detector = FileCommandMimeDetector::with_repository_and_runner(
        &repository,
        CommandRunner::new().disable_logging(true),
    );

    assert_eq!(
        None,
        detector
            .detect_file_by_content(std::path::Path::new("Cargo.toml"))
            .expect("empty file command output should be accepted")
    );
}

#[test]
fn test_file_command_detector_accessors_and_repository_only_policy() {
    let repository = MimeRepository::empty();
    let mut detector = FileCommandMimeDetector::with_repository_runner_and_config(
        &repository,
        CommandRunner::new(),
        create_precise_config(),
    );

    assert!(detector.core().media_stream_classifier().is_some());
    detector.core_mut().set_media_stream_classifier(None);
    assert!(detector.core().media_stream_classifier().is_none());
    assert_eq!(0, detector.repository().all().len());
    assert_eq!(0, detector.max_test_bytes());

    let replaced = FileCommandMimeDetector::with_repository(&repository)
        .with_command_runner(CommandRunner::new().disable_logging(true));
    assert!(replaced.command_runner().is_logging_disabled());

    assert_eq!(None, detector.detect_by_filename("unknown.bin"));
}

fn create_precise_config() -> MimeConfig {
    let mut config = qubit_config::Config::new();
    config
        .set(CONFIG_MIME_DETECTOR_DEFAULT, "file")
        .expect("detector default should be configurable");
    config
        .set(CONFIG_MEDIA_STREAM_CLASSIFIER_DEFAULT, "ffprobe")
        .expect("classifier default should be configurable");
    config
        .set(CONFIG_MIME_ENABLE_PRECISE_DETECTION, true)
        .expect("precise detection should be configurable");
    MimeConfig::from_config(&config).expect("file command config should parse")
}
