// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Runtime-created detectors retain their repository while inspecting local
//! staging.

use std::io::Write;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::options::LocalTempFileOptions;
use qubit_mime::DEFAULT_COMMAND_TIMEOUT;
use qubit_mime::MimeConfig;
use qubit_mime::MimeDetectionPolicy;
use qubit_mime::MimeRepository;
use qubit_mime::detector::MimeRuntime;

/// Explicit repository context survives runtime drop and handles a closed local
/// file.
#[test]
fn test_runtime_detector_retains_custom_repository_for_local_file() {
    let repository = MimeRepository::from_xml(
        r#"<mime-info xmlns="http://www.freedesktop.org/standards/shared-mime-info">
        <mime-type type="application/x-qubit-report"><glob pattern="*.qreport"/></mime-type>
        </mime-info>"#,
    )
    .expect("custom report repository");
    let config = MimeConfig::default();
    let expected_limit = config.max_buffer_size();
    let runtime = MimeRuntime::new(config, repository);
    assert_eq!(runtime.context().config().max_buffer_size(), expected_limit);
    assert!(
        runtime
            .context()
            .repository()
            .get("application/x-qubit-report")
            .is_some()
    );
    let cloned = runtime.clone();
    let detector = cloned.create_detector().expect("runtime detector");
    drop(cloned);
    drop(runtime);

    let filesystem = LocalFileSystem::host().expect("local filesystem");
    let mut temporary = filesystem
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_suffix(".qreport"))
        .expect("temporary report");
    temporary.write_all(b"report body").expect("report content");
    temporary.close();
    assert_eq!(detector.max_buffer_size(), expected_limit);
    assert_eq!(
        detector
            .detect_file(temporary.path(), MimeDetectionPolicy::PreferFilename)
            .expect("inspect closed temporary report")
            .as_deref(),
        Some("application/x-qubit-report")
    );
    temporary.cleanup().expect("explicit report cleanup");
}

/// Built-in and default runtime construction both produce a working detector.
#[test]
fn test_builtin_runtime_detects_content_without_registration() {
    for runtime in [
        MimeRuntime::builtin().expect("built-in runtime"),
        MimeRuntime::default(),
    ] {
        let detector = runtime.create_detector().expect("built-in detector");
        assert_eq!(
            detector
                .detect_by_content(b"%PDF-1.7\n")
                .expect("PDF detection")
                .as_deref(),
            Some("application/pdf")
        );
    }
}

#[test]
fn test_runtime_creates_detectors_using_its_repository_and_configuration() {
    let runtime = MimeRuntime::builtin().expect("built-in repository must initialize");
    assert_eq!(runtime.context().config().command_timeout(), DEFAULT_COMMAND_TIMEOUT);
    assert!(
        !runtime
            .context()
            .repository()
            .detect_by_filename("report.pdf")
            .is_empty()
    );
    let detector = runtime.create_detector().expect("runtime detector must initialize");
    drop(runtime);
    assert_eq!(
        detector
            .detect_by_filename("report.pdf")
            .expect("retained repository must remain usable"),
        Some("application/pdf".to_owned())
    );
}
