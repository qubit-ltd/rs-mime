// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::time::Duration;

#[cfg(unix)]
use qubit_command::CommandErrorKind;
use qubit_command::CommandRunner;
use qubit_config::Config;
#[cfg(unix)]
use qubit_local_files::{
    LocalFileSystem,
    LocalTempDirectoryOptions,
};
#[cfg(unix)]
use qubit_mime::MimeDetectionPolicy;
#[cfg(unix)]
use qubit_mime::MimeError;
use qubit_mime::{
    CONFIG_COMMAND_OUTPUT_MAX_BYTES,
    CONFIG_COMMAND_TIMEOUT,
    CONFIG_MEDIA_STREAM_CLASSIFIER_DEFAULT,
    CONFIG_MIME_DETECTOR_DEFAULT,
    CONFIG_MIME_ENABLE_PRECISE_DETECTION,
    DEFAULT_COMMAND_OUTPUT_MAX_BYTES,
    DEFAULT_COMMAND_TIMEOUT,
    FileBasedMimeDetector,
    FileCommandMimeDetector,
    MimeConfig,
    MimeDetector,
    MimeRepository,
};

#[cfg(unix)]
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
fn test_default_file_command_runner_uses_default_timeout() {
    let detector = FileCommandMimeDetector::new();

    assert_eq!(
        detector.command_runner().configured_timeout(),
        Some(DEFAULT_COMMAND_TIMEOUT),
    );
    assert_eq!(
        detector.command_runner().configured_max_stdout_bytes(),
        Some(DEFAULT_COMMAND_OUTPUT_MAX_BYTES),
    );
    assert_eq!(
        detector.command_runner().configured_max_stderr_bytes(),
        Some(DEFAULT_COMMAND_OUTPUT_MAX_BYTES),
    );
    assert!(
        detector
            .command_runner()
            .is_output_truncation_failure_enabled()
    );
}

#[test]
fn test_from_mime_config_limits_file_command_output() {
    let mut config = Config::new();
    config
        .set(CONFIG_COMMAND_OUTPUT_MAX_BYTES, 1024_u64)
        .expect("command output limit should be configurable");
    let detector = FileCommandMimeDetector::from_mime_config(
        MimeConfig::from_config(&config).expect("MIME config should parse"),
    );

    assert_eq!(
        Some(1024),
        detector.command_runner().configured_max_stdout_bytes()
    );
    assert_eq!(
        Some(1024),
        detector.command_runner().configured_max_stderr_bytes()
    );
    assert!(
        detector
            .command_runner()
            .is_output_truncation_failure_enabled()
    );
}

#[test]
fn test_with_repository_and_runner_uses_runner_configuration() {
    let repository = MimeRepository::empty();
    let runner =
        CommandRunner::new(Duration::from_secs(2)).disable_logging(true);
    let mut detector = FileCommandMimeDetector::with_repository_and_runner(
        &repository,
        runner,
    );

    assert_eq!(
        Some(Duration::from_secs(2)),
        detector.command_runner().configured_timeout()
    );
    assert!(detector.command_runner().is_logging_disabled());

    detector.set_command_runner(
        detector.command_runner().clone().working_directory("."),
    );
    assert_eq!(
        Some(std::path::Path::new(".")),
        detector.command_runner().configured_working_directory()
    );

    detector.set_command_runner(
        CommandRunner::new(Duration::from_secs(10)).disable_logging(false),
    );
    assert!(!detector.command_runner().is_logging_disabled());
}

#[test]
#[cfg(unix)]
fn test_detect_file_by_content_propagates_runner_timeout() {
    let temp_dir = LocalFileSystem::host()
        .create_temp_directory(&LocalTempDirectoryOptions::new())
        .expect("temporary command directory should be created");
    let script_path = temp_dir.path().join(FileCommandMimeDetector::COMMAND);
    std::fs::write(&script_path, "#!/bin/sh\nsleep 1\n")
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
        CommandRunner::new(Duration::from_millis(20)).disable_logging(true),
    );

    let error = detector
        .detect_file_by_content(std::path::Path::new("Cargo.toml"))
        .expect_err("slow fake file command should time out");

    assert!(matches!(
        error,
        MimeError::Command(ref command_error)
            if command_error.kind() == CommandErrorKind::TimedOut
    ));
}

#[test]
#[cfg(unix)]
fn test_detect_file_by_content_reads_file_command_stdout() {
    let temp_dir = LocalFileSystem::host()
        .create_temp_directory(&LocalTempDirectoryOptions::new())
        .expect("temporary command directory should be created");
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
        CommandRunner::new(Duration::from_secs(10)).disable_logging(true),
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
            .detect_reader(
                &mut reader,
                None,
                MimeDetectionPolicy::VerifyContent
            )
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
fn test_detect_file_by_content_ends_file_option_parsing_before_path() {
    let temp_dir = LocalFileSystem::host()
        .create_temp_directory(&LocalTempDirectoryOptions::new())
        .expect("temporary command directory should be created");
    let script_path = temp_dir.path().join(FileCommandMimeDetector::COMMAND);
    std::fs::write(
        &script_path,
        "#!/bin/sh\n\
         [ \"$1\" = \"--mime-type\" ] && [ \"$2\" = \"--brief\" ] && \\\n         [ \"$3\" = \"--\" ] && [ \"$4\" = \"--leading-dash\" ] || exit 9\n\
         printf 'text/plain\\n'\n",
    )
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
        CommandRunner::new(Duration::from_secs(10)).disable_logging(true),
    );

    assert_eq!(
        Some("text/plain".to_owned()),
        detector
            .detect_file_by_content(std::path::Path::new("--leading-dash"))
            .expect("file command should receive the path after its option terminator")
    );
}

#[test]
#[cfg(unix)]
fn test_detect_file_by_content_returns_none_for_empty_stdout() {
    let temp_dir = LocalFileSystem::host()
        .create_temp_directory(&LocalTempDirectoryOptions::new())
        .expect("temporary command directory should be created");
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
        CommandRunner::new(Duration::from_secs(10)).disable_logging(true),
    );

    assert_eq!(
        None,
        detector
            .detect_file_by_content(std::path::Path::new("Cargo.toml"))
            .expect("empty file command output should be accepted")
    );
}

#[test]
#[cfg(unix)]
fn test_detect_file_by_content_rejects_invalid_utf8_stdout() {
    let temp_dir = LocalFileSystem::host()
        .create_temp_directory(&LocalTempDirectoryOptions::new())
        .expect("temporary command directory should be created");
    let script_path = temp_dir.path().join(FileCommandMimeDetector::COMMAND);
    std::fs::write(&script_path, "#!/bin/sh\nprintf '\\377'\n")
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
        CommandRunner::new(Duration::from_secs(10)).disable_logging(true),
    );

    let error = detector
        .detect_file_by_content(std::path::Path::new("Cargo.toml"))
        .expect_err("invalid file UTF-8 should be reported");

    assert!(matches!(
        error,
        MimeError::DetectorBackend { backend, reason }
            if backend == "file" && reason.contains("UTF-8")
    ));
}

#[test]
#[cfg(unix)]
fn test_file_command_error_redacts_input_path() {
    let temp_dir = LocalFileSystem::host()
        .create_temp_directory(&LocalTempDirectoryOptions::new())
        .expect("temporary command directory should be created");
    let _path_guard = PathEnvGuard::set(temp_dir.path());
    let repository = MimeRepository::empty();
    let detector = FileCommandMimeDetector::with_repository(&repository);
    let private_path =
        std::path::Path::new("/private/customer/source-document.bin");

    let error = detector
        .detect_file_by_content(private_path)
        .expect_err("missing file executable should report command error");
    let display = error.to_string();
    let debug = format!("{error:?}");

    assert!(!display.contains(private_path.to_string_lossy().as_ref()));
    assert!(!debug.contains(private_path.to_string_lossy().as_ref()));
}

#[test]
fn test_file_command_detector_accessors_and_repository_only_policy() {
    let repository = MimeRepository::empty();
    let mut detector =
        FileCommandMimeDetector::with_repository_runner_and_config(
            &repository,
            CommandRunner::new(Duration::from_secs(10)),
            create_precise_config(),
        );

    detector.core_mut().set_media_stream_classifier(None);
    assert!(detector.core().media_stream_classifier().is_none());
    assert_eq!(0, detector.repository().all().len());
    assert_eq!(0, detector.max_test_bytes());

    let replaced = FileCommandMimeDetector::with_repository(&repository)
        .with_command_runner(
            CommandRunner::new(Duration::from_secs(10)).disable_logging(true),
        );
    assert!(replaced.command_runner().is_logging_disabled());

    assert_eq!(None, detector.detect_by_filename("unknown.bin"));
}

#[test]
fn test_default_file_command_runner_uses_config_timeout() {
    let mut config = Config::new();
    config
        .set(CONFIG_COMMAND_TIMEOUT, "2500ms")
        .expect("command timeout should support unit-based value");
    let detector = FileCommandMimeDetector::from_mime_config(
        MimeConfig::from_config(&config)
            .expect("file command config should parse"),
    );

    assert_eq!(
        Some(Duration::from_millis(2500)),
        detector.command_runner().configured_timeout(),
    );
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
