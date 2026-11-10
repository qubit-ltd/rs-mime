// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(unix)]
use std::process::Command;

#[cfg(unix)]
use qubit_config::Config;
#[cfg(unix)]
use qubit_local_files::LocalFileSystem;
#[cfg(unix)]
use qubit_local_files::options::LocalTempDirectoryOptions;
use qubit_mime::CONFIG_COMMAND_OUTPUT_MAX_BYTES;
use qubit_mime::CONFIG_COMMAND_TIMEOUT;
use qubit_mime::FileCommandMimeDetector;
use qubit_mime::FileCommandMimeDetectorProvider;
use qubit_mime::MimeConfig;
use qubit_spi::ProviderMetadata;
use qubit_spi::ServiceProvider;
#[cfg(unix)]
use qubit_spi::error::ProviderFailureKind;

#[cfg(unix)]
use crate::support::PathEnvGuard;

#[test]
fn test_file_command_mime_detector_provider_metadata_and_availability() {
    #[cfg(unix)]
    let _path_guard = PathEnvGuard::preserve();
    let provider = FileCommandMimeDetectorProvider;
    let descriptor = provider.descriptor();
    let creation = provider.create_configured(&MimeConfig::default());

    assert_eq!("file", descriptor.id().as_str());
    assert_eq!(
        vec!["file-command", "file-command-mime-detector"],
        descriptor
            .aliases()
            .iter()
            .map(|alias| alias.as_str())
            .collect::<Vec<_>>()
    );
    assert_eq!(10, descriptor.priority());
    assert_eq!(FileCommandMimeDetector::is_available(), creation.is_ok());
}

#[cfg(unix)]
#[test]
fn test_file_provider_applies_probe_output_and_timeout_budgets() {
    let directory = LocalFileSystem::host()
        .expect("host filesystem should open")
        .create_temp_directory_with_options(&LocalTempDirectoryOptions::new())
        .expect("temporary command directory should be created");
    let executable = directory.path().join("file");
    let _path_guard = PathEnvGuard::prepend(directory.path());
    let write_command = |body: &str| {
        std::fs::write(&executable, body).expect("fake file command should be written");
        let mut permissions = std::fs::metadata(&executable)
            .expect("fake file command metadata should be readable")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&executable, permissions).expect("fake file command should be executable");
    };
    let provider = FileCommandMimeDetectorProvider;

    write_command("#!/bin/sh\nprintf 'application/octet-stream\\n'\n");
    let mut source = Config::new();
    source
        .set(CONFIG_COMMAND_OUTPUT_MAX_BYTES, 1_u64)
        .expect("small output cap should be accepted");
    let small_output_config = MimeConfig::from_config(&source).expect("small output config should parse");
    assert_eq!(
        ProviderFailureKind::Unavailable,
        provider
            .create_configured(&small_output_config)
            .expect_err("truncated probe output should report unavailable")
            .kind()
    );

    write_command("#!/bin/sh\nsleep 1\nprintf 'application/octet-stream\\n'\n");
    let mut source = Config::new();
    source
        .set(CONFIG_COMMAND_TIMEOUT, "30ms")
        .expect("short timeout should be accepted");
    let short_timeout_config = MimeConfig::from_config(&source).expect("short timeout config should parse");
    assert_eq!(
        ProviderFailureKind::Unavailable,
        provider
            .create_configured(&short_timeout_config)
            .expect_err("timed out probe should report unavailable")
            .kind()
    );
}

#[cfg(unix)]
#[test]
fn test_file_command_provider_reports_unavailable_command() {
    const CHILD_MARKER: &str = "QUBIT_MIME_TEST_MISSING_FILE_COMMAND";
    if std::env::var_os(CHILD_MARKER).is_some() {
        let error = FileCommandMimeDetectorProvider
            .create_configured(&MimeConfig::default())
            .expect_err("provider should reject a missing file command");

        assert_eq!(ProviderFailureKind::Unavailable, error.kind());
        return;
    }

    let current_test = std::env::current_exe().expect("current integration test executable should be available");
    let test_name =
        "detector::file_command_mime_detector_provider_tests::test_file_command_provider_reports_unavailable_command";
    let status = Command::new(current_test)
        .args(["--exact", test_name, "--nocapture"])
        .env(CHILD_MARKER, "1")
        .env("PATH", "/qubit-mime-test-missing-command")
        .status()
        .expect("isolated provider test should start");

    assert!(status.success(), "isolated provider test should pass");
}
