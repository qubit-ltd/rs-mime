// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[cfg(unix)]
use qubit_config::Config;
#[cfg(unix)]
use qubit_local_files::LocalFileSystem;
#[cfg(unix)]
use qubit_local_files::options::LocalTempDirectoryOptions;
use qubit_mime::CONFIG_COMMAND_OUTPUT_MAX_BYTES;
use qubit_mime::CONFIG_COMMAND_TIMEOUT;
use qubit_mime::FfprobeCommandMediaStreamClassifierProvider;
use qubit_mime::MimeConfig;
use qubit_spi::ProviderMetadata;
#[cfg(unix)]
use qubit_spi::ServiceProvider;
#[cfg(unix)]
use qubit_spi::error::ProviderFailureKind;

#[cfg(unix)]
use crate::support::PathEnvGuard;

#[test]
fn test_default_provider_has_the_ffprobe_identity() {
    let provider = FfprobeCommandMediaStreamClassifierProvider;

    assert_eq!("ffprobe", provider.descriptor().id().as_str());
}

#[cfg(unix)]
#[test]
fn test_ffprobe_provider_applies_probe_output_and_timeout_budgets() {
    let directory = LocalFileSystem::host()
        .expect("host filesystem should open")
        .create_temp_directory_with_options(&LocalTempDirectoryOptions::new())
        .expect("temporary command directory should be created");
    let executable = directory.path().join("ffprobe");
    let _path_guard = PathEnvGuard::prepend(directory.path());
    let write_command = |body: &str| {
        std::fs::write(&executable, body).expect("fake ffprobe should be written");
        let mut permissions = std::fs::metadata(&executable)
            .expect("fake ffprobe metadata should be readable")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&executable, permissions).expect("fake ffprobe should be executable");
    };
    let provider = FfprobeCommandMediaStreamClassifierProvider;

    write_command("#!/bin/sh\nprintf 'available\\n'\n");
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

    write_command("#!/bin/sh\nsleep 1\nprintf 'available\\n'\n");
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
