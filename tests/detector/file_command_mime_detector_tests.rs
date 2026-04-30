/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/

use std::time::Duration;

use qubit_command::CommandRunner;
use qubit_mime::{FileCommandMimeDetector, MimeDetector, MimeRepository};

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
        .disable_logging(true)
        .lossy_output(true);
    let mut detector = FileCommandMimeDetector::with_repository_and_runner(&repository, runner);

    assert_eq!(
        Some(Duration::from_secs(2)),
        detector.command_runner().configured_timeout()
    );
    assert!(detector.is_disable_logging());
    assert!(detector.command_runner().is_lossy_output_enabled());

    detector.set_working_directory(".");
    assert_eq!(
        Some(std::path::Path::new(".")),
        detector.working_directory()
    );

    detector.set_command_runner(CommandRunner::new().disable_logging(false));
    assert!(!detector.is_disable_logging());
}

#[test]
fn test_detect_path_by_content_honors_execution_timeout() {
    if !FileCommandMimeDetector::is_available() {
        return;
    }
    let mut detector = FileCommandMimeDetector::new();
    detector.set_execution_timeout(Duration::ZERO);

    assert!(detector.detect_path_by_content("Cargo.toml").is_err());
}
