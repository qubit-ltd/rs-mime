// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

#[cfg(unix)]
use qubit_command::CommandErrorKind;
use qubit_command::CommandRunner;
use qubit_config::Config;
#[cfg(unix)]
use qubit_local_files::LocalFileSystem;
#[cfg(unix)]
use qubit_local_files::options::LocalTempDirectoryOptions;
use qubit_mime::CONFIG_COMMAND_OUTPUT_MAX_BYTES;
use qubit_mime::CONFIG_COMMAND_TIMEOUT;
use qubit_mime::CONFIG_MEDIA_STREAM_CLASSIFIER_DEFAULT;
use qubit_mime::CONFIG_MIME_DETECTOR_DEFAULT;
use qubit_mime::CONFIG_MIME_ENABLE_PRECISE_DETECTION;
use qubit_mime::CONFIG_MIME_MAX_BUFFER_SIZE;
use qubit_mime::DEFAULT_COMMAND_OUTPUT_MAX_BYTES;
use qubit_mime::DEFAULT_COMMAND_TIMEOUT;
use qubit_mime::FileBasedMimeDetector;
use qubit_mime::FileCommandMimeDetector;
use qubit_mime::MimeConfig;
#[cfg(unix)]
use qubit_mime::MimeDetectionPolicy;
use qubit_mime::MimeDetector;
#[cfg(unix)]
use qubit_mime::MimeError;
use qubit_mime::MimeRepository;

#[test]
fn test_custom_repository_relations() {
    let repository = MimeRepository::from_xml(
        r#"<mime-info xmlns="http://www.freedesktop.org/standards/shared-mime-info">
  <mime-type type="application/x-review-report">
    <glob pattern="*.review-report"/>
    <alias type="application/x-review-report-alias"/>
    <sub-class-of type="application/pdf"/>
  </mime-type>
</mime-info>"#,
    )
    .expect("custom repository should parse");
    let detector = FileCommandMimeDetector::with_repository(&repository);
    let filename = vec!["application/x-review-report".to_owned()];
    let content = vec!["application/pdf".to_owned()];
    assert_eq!(
        filename.first().cloned(),
        detector.core().merge_results(&filename, &content)
    );
    let aliases = vec!["application/x-review-report-alias".to_owned()];
    assert_eq!(
        aliases.first().cloned(),
        detector.core().merge_results(&aliases, &content)
    );
}

#[test]
fn test_empty_repository_relations() {
    let repository = MimeRepository::empty();
    let detector = FileCommandMimeDetector::with_repository(&repository);
    let filename = vec!["text/html".to_owned()];
    let content = vec!["text/plain".to_owned()];
    assert_eq!(
        content.first().cloned(),
        detector.core().merge_results(&filename, &content)
    );
}

#[cfg(unix)]
use crate::support::PathEnvGuard;

#[test]
fn test_detect_by_filename_uses_repository_candidates() {
    let detector = FileCommandMimeDetector::new();

    assert_eq!(
        Some("image/jpeg".to_owned()),
        detector
            .detect_by_filename("photo.jpg")
            .expect("filename detection should succeed")
    );
}

#[test]
fn test_is_available_can_be_called_without_panicking() {
    let _ = FileCommandMimeDetector::is_available();
}

#[test]
fn test_default_file_command_runner_uses_default_timeout() {
    let detector = FileCommandMimeDetector::from_mime_config(
        MimeConfig::from_config(&Config::new()).expect("empty config should use defaults"),
    );

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
    assert!(detector.command_runner().is_output_truncation_failure_enabled());
}

#[test]
fn test_from_mime_config_limits_file_command_output() {
    let mut config = Config::new();
    config
        .set(CONFIG_COMMAND_OUTPUT_MAX_BYTES, 1024_u64)
        .expect("command output limit should be configurable");
    let detector =
        FileCommandMimeDetector::from_mime_config(MimeConfig::from_config(&config).expect("MIME config should parse"));

    assert_eq!(Some(1024), detector.command_runner().configured_max_stdout_bytes());
    assert_eq!(Some(1024), detector.command_runner().configured_max_stderr_bytes());
    assert!(detector.command_runner().is_output_truncation_failure_enabled());
}

#[test]
fn test_with_repository_and_runner_uses_runner_configuration() {
    let repository = MimeRepository::empty();
    let runner = CommandRunner::new(Duration::from_secs(2)).disable_logging(true);
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

    detector.set_command_runner(CommandRunner::new(Duration::from_secs(10)).disable_logging(false));
    assert!(!detector.command_runner().is_logging_disabled());
}

#[test]
#[cfg(unix)]
fn test_detect_file_by_content_propagates_runner_timeout() {
    let temp_dir = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .create_temp_directory_with_options(&LocalTempDirectoryOptions::new())
        .expect("temporary command directory should be created");
    let script_path = temp_dir.path().join(FileCommandMimeDetector::COMMAND);
    std::fs::write(&script_path, "#!/bin/sh\nsleep 1\n").expect("fake file command should be written");
    let mut permissions = std::fs::metadata(&script_path)
        .expect("fake file command metadata should be readable")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&script_path, permissions).expect("fake file command should be executable");
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
        .expect("Host filesystem should open")
        .create_temp_directory_with_options(&LocalTempDirectoryOptions::new())
        .expect("temporary command directory should be created");
    let script_path = temp_dir.path().join(FileCommandMimeDetector::COMMAND);
    std::fs::write(&script_path, "#!/bin/sh\nprintf 'text/plain\\n'\n").expect("fake file command should be written");
    let mut permissions = std::fs::metadata(&script_path)
        .expect("fake file command metadata should be readable")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&script_path, permissions).expect("fake file command should be executable");
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
        detector
            .detect_by_content(b"plain text")
            .expect("content detection should succeed")
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
            .detect_file(std::path::Path::new("Cargo.toml"), MimeDetectionPolicy::PreferFilename,)
            .expect("fake file command should support full file detection")
    );
}

#[test]
#[cfg(unix)]
fn test_detect_file_by_content_ends_file_option_parsing_before_path() {
    let temp_dir = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .create_temp_directory_with_options(&LocalTempDirectoryOptions::new())
        .expect("temporary command directory should be created");
    let script_path = temp_dir.path().join(FileCommandMimeDetector::COMMAND);
    std::fs::write(
        &script_path,
        "#!/bin/sh\n\
         [ \"$1\" = \"--mime-type\" ] && [ \"$2\" = \"--brief\" ] && \\\n         [ \"$3\" = \"--\" ] && [ \"$4\" = \"$PWD/--leading-dash\" ] || exit 9\n\
         printf 'text/plain\\n'\n",
    )
    .expect("fake file command should be written");
    let mut permissions = std::fs::metadata(&script_path)
        .expect("fake file command metadata should be readable")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&script_path, permissions).expect("fake file command should be executable");
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
        .expect("Host filesystem should open")
        .create_temp_directory_with_options(&LocalTempDirectoryOptions::new())
        .expect("temporary command directory should be created");
    let script_path = temp_dir.path().join(FileCommandMimeDetector::COMMAND);
    std::fs::write(&script_path, "#!/bin/sh\nexit 0\n").expect("fake file command should be written");
    let mut permissions = std::fs::metadata(&script_path)
        .expect("fake file command metadata should be readable")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&script_path, permissions).expect("fake file command should be executable");
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
        .expect("Host filesystem should open")
        .create_temp_directory_with_options(&LocalTempDirectoryOptions::new())
        .expect("temporary command directory should be created");
    let script_path = temp_dir.path().join(FileCommandMimeDetector::COMMAND);
    std::fs::write(&script_path, "#!/bin/sh\nprintf '\\377'\n").expect("fake file command should be written");
    let mut permissions = std::fs::metadata(&script_path)
        .expect("fake file command metadata should be readable")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&script_path, permissions).expect("fake file command should be executable");
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
        .expect("Host filesystem should open")
        .create_temp_directory_with_options(&LocalTempDirectoryOptions::new())
        .expect("temporary command directory should be created");
    let _path_guard = PathEnvGuard::set(temp_dir.path());
    let repository = MimeRepository::empty();
    let detector = FileCommandMimeDetector::with_repository(&repository);
    let private_path = std::path::Path::new("/private/customer/source-document.bin");

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
    let mut detector = FileCommandMimeDetector::with_repository_runner_and_config(
        &repository,
        CommandRunner::new(Duration::from_secs(10)),
        create_precise_config(),
    );

    detector.core_mut().set_media_stream_classifier(None);
    assert!(detector.core().media_stream_classifier().is_none());
    assert_eq!(0, detector.repository().all().len());
    assert_eq!(detector.core().max_buffer_size(), detector.max_test_bytes());

    let replaced = FileCommandMimeDetector::with_repository(&repository)
        .with_command_runner(CommandRunner::new(Duration::from_secs(10)).disable_logging(true));
    assert!(replaced.command_runner().is_logging_disabled());

    assert_eq!(
        None,
        detector
            .detect_by_filename("unknown.bin")
            .expect("filename detection should succeed")
    );
}

#[test]
fn test_file_reader_staging_budget_from_config() {
    let mut source = Config::new();
    source
        .set(CONFIG_MIME_MAX_BUFFER_SIZE, 8_u64)
        .expect("reader budget should be configurable");
    #[cfg(not(unix))]
    {
        let repository = MimeRepository::empty();
        let detector = FileCommandMimeDetector::with_repository_runner_and_config(
            &repository,
            CommandRunner::new(Duration::from_secs(10)).disable_logging(true),
            MimeConfig::from_config(&source).expect("MIME config should parse"),
        );
        assert_eq!(8, FileBasedMimeDetector::max_test_bytes(&detector));
    }

    #[cfg(unix)]
    {
        let directory = LocalFileSystem::host()
            .expect("host filesystem available")
            .create_temp_directory_with_options(&LocalTempDirectoryOptions::new())
            .expect("fixture directory created");
        let script = directory.path().join("file");
        std::fs::write(
            &script,
            "#!/bin/sh\nfor input_path do :; done\nif [ \"$(cat \"$input_path\")\" = \"%PDF-1.7\" ]; then printf 'application/pdf\\n'; else printf 'application/octet-stream\\n'; fi\n",
        )
        .expect("fake file command written");
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
            .expect("fake file command executable");
        let _path_guard = PathEnvGuard::prepend(directory.path());

        let input = b"%PDF-1.7TAIL";
        for repository_xml in [
            None,
            Some(
                r#"<mime-info xmlns="http://www.freedesktop.org/standards/shared-mime-info"><mime-type type="application/x-glob"><glob pattern="*.glob"/></mime-type></mime-info>"#,
            ),
            Some(
                r#"<mime-info xmlns="http://www.freedesktop.org/standards/shared-mime-info"><mime-type type="application/x-magic"><magic><match type="string" value="P" offset="0"/></magic></mime-type></mime-info>"#,
            ),
        ] {
            let repository = repository_xml
                .map(MimeRepository::from_xml)
                .transpose()
                .expect("fixture repository should parse")
                .unwrap_or_else(MimeRepository::empty);
            let detector = FileCommandMimeDetector::with_repository_runner_and_config(
                &repository,
                CommandRunner::new(Duration::from_secs(10)).disable_logging(true),
                MimeConfig::from_config(&source).expect("MIME config should parse"),
            );
            assert_eq!(
                Some("application/pdf".to_owned()),
                detector
                    .detect_reader(
                        &mut std::io::Cursor::new(input),
                        None,
                        MimeDetectionPolicy::VerifyContent
                    )
                    .expect("reader command should classify its eight staged bytes"),
            );
        }

        let mut source = Config::new();
        source
            .set(CONFIG_MIME_MAX_BUFFER_SIZE, 32_u64)
            .expect("larger budget should be configurable");
        let config = MimeConfig::from_config(&source).expect("MIME config should parse");
        let repository = MimeRepository::empty();
        let detector = FileCommandMimeDetector::with_repository_runner_and_config(
            &repository,
            CommandRunner::new(Duration::from_secs(10)).disable_logging(true),
            config,
        );
        let short = b"%PDF-1.7";
        let by_content = detector
            .detect_by_content(short)
            .expect("bytes detection should succeed");
        let by_reader = detector
            .detect_reader(
                &mut std::io::Cursor::new(short.to_vec()),
                None,
                MimeDetectionPolicy::VerifyContent,
            )
            .expect("reader detection should succeed");
        let short_path = directory.path().join("short-input");
        std::fs::write(&short_path, short).expect("short input written");
        let by_file = detector
            .detect_file(&short_path, MimeDetectionPolicy::VerifyContent)
            .expect("local file detection should succeed");
        assert_eq!(by_content, by_reader);
        assert_eq!(by_reader, by_file);

        let mut cursor = std::io::Cursor::new(short.to_vec());
        cursor.set_position(3);
        assert_eq!(
            Some("application/pdf".to_owned()),
            detector
                .detect_reader(&mut cursor, None, MimeDetectionPolicy::VerifyContent)
                .expect("reader detection should inspect from zero and restore position"),
        );
        assert_eq!(3, cursor.position());

        let mut zero_source = Config::new();
        zero_source
            .set(CONFIG_MIME_MAX_BUFFER_SIZE, 0_u64)
            .expect("zero budget should be configurable");
        let zero_config = MimeConfig::from_config(&zero_source).expect("zero config should parse");
        let zero_detector = FileCommandMimeDetector::with_repository_runner_and_config(
            &repository,
            CommandRunner::new(Duration::from_secs(10)).disable_logging(true),
            zero_config,
        );
        std::fs::write(
            &script,
            "#!/bin/sh\nfor input_path do :; done\nif [ ! -s \"$input_path\" ]; then printf 'application/pdf\\n'; else printf 'application/octet-stream\\n'; fi\n",
        )
        .expect("zero-budget fake command written");
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
            .expect("zero-budget fake command executable");
        assert_eq!(
            Some("application/pdf".to_owned()),
            zero_detector
                .detect_reader(
                    &mut std::io::Cursor::new(input),
                    None,
                    MimeDetectionPolicy::VerifyContent
                )
                .expect("zero-budget reader staging should be empty"),
        );

        let custom_repository = MimeRepository::from_xml(
            r#"<mime-info xmlns="http://www.freedesktop.org/standards/shared-mime-info"><mime-type type="application/x-review-report"><glob pattern="*.review-report"/><sub-class-of type="application/pdf"/></mime-type></mime-info>"#,
        )
        .expect("custom repository should parse");
        std::fs::write(
            &script,
            "#!/bin/sh\nfor input_path do :; done\nif [ \"$(cat \"$input_path\")\" = \"%PDF-1.7\" ]; then printf 'application/pdf\\n'; else printf 'application/octet-stream\\n'; fi\n",
        )
        .expect("custom-relation fake command written");
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
            .expect("custom-relation fake command executable");
        let custom_detector = FileCommandMimeDetector::with_repository_runner_and_config(
            &custom_repository,
            CommandRunner::new(Duration::from_secs(10)).disable_logging(true),
            MimeConfig::from_config(&source).expect("MIME config should parse"),
        );
        assert_eq!(
            Some("application/x-review-report".to_owned()),
            custom_detector
                .detect_reader(
                    &mut std::io::Cursor::new(short.to_vec()),
                    Some("report.review-report"),
                    MimeDetectionPolicy::Freedesktop,
                )
                .expect("Freedesktop policy should merge custom inheritance"),
        );
        assert_eq!(
            Some("application/pdf".to_owned()),
            custom_detector
                .detect_reader(
                    &mut std::io::Cursor::new(short.to_vec()),
                    Some("report.review-report"),
                    MimeDetectionPolicy::ContentFirst,
                )
                .expect("ContentFirst should prefer the command result"),
        );
        assert_eq!(32, FileBasedMimeDetector::max_test_bytes(&detector));
    }
}

#[test]
fn test_default_file_command_runner_uses_config_timeout() {
    let mut config = Config::new();
    config
        .set(CONFIG_COMMAND_TIMEOUT, "2500ms")
        .expect("command timeout should support unit-based value");
    let detector = FileCommandMimeDetector::from_mime_config(
        MimeConfig::from_config(&config).expect("file command config should parse"),
    );

    assert_eq!(
        Some(Duration::from_millis(2500)),
        detector.command_runner().configured_timeout(),
    );
}

fn create_precise_config() -> MimeConfig {
    let mut config = Config::new();
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

#[test]
#[cfg(unix)]
fn test_file_command_contract_checks_truncation_and_actual_exit_status() {
    let directory = LocalFileSystem::host()
        .expect("host filesystem available")
        .create_temp_directory_with_options(&LocalTempDirectoryOptions::new())
        .expect("fixture directory created");
    let script = directory.path().join("file");
    std::fs::write(&script, "#!/bin/sh\nprintf text/plain\n").expect("script written");
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("script executable");
    let _guard = PathEnvGuard::prepend(directory.path());
    let runner = CommandRunner::new(DEFAULT_COMMAND_TIMEOUT).disable_logging(true);
    let detector = FileCommandMimeDetector::default()
        .with_command_runner(runner.clone().max_stdout_bytes(4).fail_on_output_truncation(false));
    assert!(matches!(
        detector.detect_file_by_content(std::path::Path::new("input")),
        Err(MimeError::DetectorBackend { .. })
    ));
    let detector = FileCommandMimeDetector::default().with_command_runner(runner.clone().success_exit_codes(&[7]));
    assert_eq!(
        detector
            .detect_file_by_content(std::path::Path::new("input"))
            .expect("actual zero must parse"),
        Some("text/plain".to_owned())
    );
    // Replace the inode instead of truncating a recently executed script.
    let replacement = directory.path().join("file-next");
    std::fs::write(&replacement, "#!/bin/sh\nprintf text/plain\nexit 7\n").expect("nonzero script written");
    std::fs::set_permissions(&replacement, std::fs::Permissions::from_mode(0o755)).expect("replacement executable");
    std::fs::rename(&replacement, &script).expect("nonzero script installed");
    let detector = FileCommandMimeDetector::default().with_command_runner(runner.success_exit_codes(&[0, 7]));
    assert!(matches!(
        detector.detect_file_by_content(std::path::Path::new("input")),
        Err(MimeError::DetectorBackend { .. })
    ));
}

#[test]
#[cfg(unix)]
fn test_detect_file_rejects_successful_command_error_text() {
    let directory = LocalFileSystem::host()
        .expect("host filesystem available")
        .create_temp_directory_with_options(&LocalTempDirectoryOptions::new())
        .expect("fixture directory created");
    let script = directory.path().join("file");
    std::fs::write(&script, "#!/bin/sh\nprintf 'cannot open private-media\\n'\nexit 0\n")
        .expect("fake utility written");
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("script executable");
    let _guard = PathEnvGuard::prepend(directory.path());
    let detector = FileCommandMimeDetector::new()
        .with_command_runner(CommandRunner::new(DEFAULT_COMMAND_TIMEOUT).disable_logging(true));
    let error = detector
        .detect_file(
            std::path::Path::new("private-media"),
            MimeDetectionPolicy::VerifyContent,
        )
        .expect_err("successful utility error text must be rejected");
    assert!(matches!(error, MimeError::DetectorBackend { ref backend, ref reason }
        if backend == "file" && reason == "file returned an invalid MIME type"
            && !reason.contains("private-media")));
    assert!(!format!("{error:?}").contains("private-media"));
}

#[test]
fn test_detect_file_reports_missing_input_with_host_file_utility() {
    let _path_guard = PathEnvGuard::preserve();
    if !FileCommandMimeDetector::is_available() {
        eprintln!("skipping: host file utility is unavailable");
        return;
    }
    let detector = FileCommandMimeDetector::new();
    let error = detector
        .detect_file_by_content(std::path::Path::new("qubit-mime-input-that-does-not-exist"))
        .expect_err("host file utility should reject a missing input");
    assert!(matches!(
        error,
        MimeError::DetectorBackend { .. } | MimeError::Command(_)
    ));
}
