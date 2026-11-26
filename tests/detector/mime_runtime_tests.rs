// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Runtime-created detectors retain their repository while inspecting local
//! staging.

use std::sync::Arc;

use qubit_io::Output;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::options::LocalTempFileOptions;
use qubit_mime::DEFAULT_COMMAND_TIMEOUT;
use qubit_mime::MimeConfig;
use qubit_mime::MimeDetectionPolicy;
use qubit_mime::MimeDetector;
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
    let repository = Arc::new(repository);
    let weak = Arc::downgrade(&repository);
    let runtime = MimeRuntime::new(config, Arc::clone(&repository));
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
    drop(repository);
    assert!(weak.upgrade().is_some(), "detector should retain its repository");

    let filesystem = LocalFileSystem::host().expect("local filesystem");
    let mut temporary = filesystem
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_suffix(".qreport"))
        .expect("temporary report");
    Output::write_fully(&mut temporary, b"report body").expect("report content");
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
    drop(detector);
    assert!(weak.upgrade().is_none(), "repository should release with the detector");
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

#[test]
fn test_runtime_debug_reports_optional_managed_services() {
    let runtime = MimeRuntime::builtin().expect("built-in repository must initialize");
    let debug = format!("{runtime:?}");

    assert!(debug.contains("MimeRuntime"));
    assert!(debug.contains("has_execution_services: false"));
    assert!(debug.contains("has_async_backend: false"));
}

#[test]
fn test_runtime_debug_reports_configured_execution_services() {
    #[derive(Debug)]
    struct NoopExecution;

    impl qubit_mime::MimeExecutionPort for NoopExecution {
        #[inline]
        fn prepare(&self) -> Result<(), qubit_mime::MimeError> {
            Ok(())
        }

        #[inline]
        fn shutdown(&self) -> Result<(), qubit_mime::MimeError> {
            Ok(())
        }
    }

    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let group = manager.group(qubit_operation::GroupOptions { capacity: 1 }).unwrap();
    let services = qubit_mime::MimeExecutionServices::new(manager, group, Arc::new(NoopExecution), 1024).unwrap();
    let runtime = MimeRuntime::builtin()
        .expect("built-in repository must initialize")
        .with_execution_services(services);
    let debug = format!("{runtime:?}");

    assert!(debug.contains("has_execution_services: true"));
}

#[test]
fn test_runtime_detector_uses_only_its_custom_repository_relations() {
    let repository = MimeRepository::from_xml(
        r#"<mime-info xmlns="http://www.freedesktop.org/standards/shared-mime-info">
<mime-type type="application/x-runtime-child">
  <glob pattern="*.rpt"/>
  <sub-class-of type="application/x-runtime-parent"/>
</mime-type>
<mime-type type="application/x-runtime-sibling"><glob pattern="*.rpt"/></mime-type>
<mime-type type="application/x-runtime-parent">
  <magic priority="80"><match type="string" value="RUNTIME" offset="0"/></magic>
</mime-type>
</mime-info>"#,
    )
    .expect("custom runtime repository should parse");
    let runtime = MimeRuntime::new(MimeConfig::default(), Arc::new(repository));
    let detector = runtime.create_detector().expect("runtime detector");

    assert_eq!(
        Some("application/x-runtime-child".to_owned()),
        detector
            .detect(b"RUNTIME", Some("summary.rpt"), MimeDetectionPolicy::Freedesktop)
            .expect("runtime detection should succeed")
    );
}

#[test]
fn test_runtime_provider_selection_keeps_custom_repository_in_sync_facade() {
    #[derive(Debug)]
    struct NoopExecution;
    impl qubit_mime::MimeExecutionPort for NoopExecution {
        fn prepare(&self) -> Result<(), qubit_mime::MimeError> {
            Ok(())
        }
        fn shutdown(&self) -> Result<(), qubit_mime::MimeError> {
            Ok(())
        }
    }

    let repository = Arc::new(MimeRepository::from_xml(
        r#"<mime-info xmlns="http://www.freedesktop.org/standards/shared-mime-info">
        <mime-type type="application/x-runtime-only"><glob pattern="*.runtime-only"/><magic priority="80"><match type="string" value="RUNTIME-ONLY" offset="0"/></magic></mime-type>
        </mime-info>"#,
    )
    .unwrap());
    let manager = qubit_operation::OperationManager::new(Default::default()).unwrap();
    let group = manager.group(qubit_operation::GroupOptions { capacity: 2 }).unwrap();
    let services = qubit_mime::MimeExecutionServices::new(manager, group, Arc::new(NoopExecution), 1024).unwrap();
    let runtime = MimeRuntime::new(MimeConfig::default(), repository)
        .with_execution_services(services.clone())
        .with_async_provider_selection_and_classifier(
            &qubit_mime::MimeDetectorRegistry::builtin(),
            &qubit_spi::ProviderSelection::named("repository").unwrap(),
            &qubit_mime::MediaStreamClassifierRegistry::builtin(),
            &qubit_spi::ProviderSelection::named("ffprobe").unwrap(),
        )
        .expect("explicit async detector and classifier selection");

    let detector = runtime
        .create_detector()
        .expect("synchronous facade after classifier selection");
    let runtime = runtime.with_execution_services(services);
    let detector_after_reattach = runtime
        .create_detector()
        .expect("synchronous facade after reattaching services");
    assert_eq!(
        detector.detect_by_filename("report.runtime-only").unwrap(),
        Some("application/x-runtime-only".to_owned())
    );
    assert_eq!(
        detector_after_reattach
            .detect_by_filename("report.runtime-only")
            .unwrap(),
        Some("application/x-runtime-only".to_owned())
    );
    assert_eq!(
        detector.detect_by_content(b"RUNTIME-ONLY body").unwrap(),
        Some("application/x-runtime-only".to_owned())
    );
    assert_eq!(detector.detect_by_filename("report.pdf").unwrap(), None);
}
