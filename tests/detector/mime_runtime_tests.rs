// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Verifies a runtime creates a usable detector from its retained repository.
use qubit_mime::DEFAULT_COMMAND_TIMEOUT;
use qubit_mime::detector::MimeRuntime;

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
