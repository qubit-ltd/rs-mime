// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#[cfg(unix)]
use std::process::Command;

use qubit_mime::{
    FileCommandMimeDetector,
    FileCommandMimeDetectorProvider,
    MimeConfig,
};
#[cfg(unix)]
use qubit_spi::error::ProviderFailureKind;
use qubit_spi::{
    ProviderMetadata,
    ServiceProvider,
};

use crate::support::PathEnvGuard;

#[test]
fn test_file_command_mime_detector_provider_metadata_and_availability() {
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
fn test_file_command_provider_reports_unavailable_command() {
    const CHILD_MARKER: &str = "QUBIT_MIME_TEST_MISSING_FILE_COMMAND";
    if std::env::var_os(CHILD_MARKER).is_some() {
        let error = FileCommandMimeDetectorProvider
            .create_configured(&MimeConfig::default())
            .expect_err("provider should reject a missing file command");

        assert_eq!(ProviderFailureKind::Unavailable, error.kind());
        return;
    }

    let current_test = std::env::current_exe()
        .expect("current integration test executable should be available");
    let test_name = "detector::file_command_mime_detector_provider_tests::test_file_command_provider_reports_unavailable_command";
    let status = Command::new(current_test)
        .args(["--exact", test_name, "--nocapture"])
        .env(CHILD_MARKER, "1")
        .env("PATH", "/qubit-mime-test-missing-command")
        .status()
        .expect("isolated provider test should start");

    assert!(status.success(), "isolated provider test should pass");
}
