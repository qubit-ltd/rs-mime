// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Staging failures that public inspection callbacks cannot manufacture.

use std::fs;
use std::io;
use std::io::Write;

use qubit_local_files::error::LocalFileOperation;

use crate::MimeError;
use crate::temp_staging::with_temp_file;

/// Early staging failures still explicitly release both file and sandbox.
#[test]
fn test_failed_staging_cleans_resource_and_skips_inspection() {
    let mut recorded = None;
    let error = with_temp_file::<()>(
        "StagingFailure-",
        |file| {
            recorded = Some(file.path().to_path_buf());
            file.write_all(b"partial")?;
            Err(MimeError::Io(io::Error::from(io::ErrorKind::UnexpectedEof)))
        },
        |_| panic!("failed staging must not invoke inspection"),
    )
    .expect_err("staging must fail");
    assert!(matches!(error, MimeError::Io(ref error) if error.kind() == io::ErrorKind::UnexpectedEof));
    let path = recorded.expect("staged path should be captured");
    assert!(!path.exists());
    assert!(!path.parent().expect("sandbox exists in path").exists());
}

/// Staging and cleanup errors remain independently accessible to recovery code.
#[test]
fn test_failed_staging_retains_secondary_cleanup_error() {
    let mut recorded = None;
    let error = with_temp_file::<()>(
        "StagingCleanupFailure-",
        |file| {
            let sandbox = file.path().parent().expect("sandbox should exist");
            fs::write(sandbox.join("retained"), b"test-owned blocker")?;
            recorded = Some(file.path().to_path_buf());
            Err(MimeError::InvalidClassifierInput {
                reason: "input failed".to_owned(),
            })
        },
        |_| panic!("failed staging must not invoke inspection"),
    )
    .expect_err("staging must fail");
    let path = recorded.expect("staged path should be captured");
    let sandbox = path.parent().expect("sandbox should exist in path");
    assert!(!path.exists(), "explicit cleanup should have removed the payload");
    fs::remove_file(sandbox.join("retained")).expect("test blocker should be removed");
    fs::remove_dir(sandbox).expect("residual sandbox should be removed");
    match error {
        MimeError::TemporaryCleanup { primary, cleanup } => {
            assert!(matches!(*primary, MimeError::InvalidClassifierInput { ref reason } if reason == "input failed"));
            assert_eq!(LocalFileOperation::Cleanup, cleanup.operation());
            assert_eq!(Some(sandbox), cleanup.path());
        }
        other => panic!("expected both errors, got {other}"),
    }
}

/// Successful inspection still reports a cleanup-only failure as an I/O error.
#[test]
fn test_successful_inspection_reports_cleanup_failure() {
    let mut recorded = None;
    let error = with_temp_file(
        "CleanupFailure-",
        |file| {
            file.write_all(b"payload")?;
            Ok(())
        },
        |path| {
            assert_eq!(b"payload", fs::read(path)?.as_slice());
            let sandbox = path.parent().expect("sandbox should exist");
            fs::write(sandbox.join("retained"), b"test-owned blocker")?;
            recorded = Some(path.to_path_buf());
            Ok(42)
        },
    )
    .expect_err("cleanup should fail");
    let path = recorded.expect("staged path should be captured");
    assert!(!path.exists());
    let sandbox = path.parent().expect("sandbox should exist in path");
    fs::remove_file(sandbox.join("retained")).expect("test blocker should be removed");
    fs::remove_dir(sandbox).expect("residual sandbox should be removed");
    assert!(matches!(error, MimeError::Io(_)));
}

/// Inspection errors survive cleanup, including a simultaneous sandbox failure.
#[test]
fn test_failed_inspection_retains_primary_and_cleanup_context() {
    for block_cleanup in [false, true] {
        let mut recorded = None;
        let error = with_temp_file::<()>(
            "InspectionFailure-",
            |file| {
                file.write_all(b"payload")?;
                Ok(())
            },
            |path| {
                assert_eq!(fs::read(path)?, b"payload");
                recorded = Some(path.to_path_buf());
                if block_cleanup {
                    fs::write(path.parent().expect("sandbox path").join("retained"), b"test blocker")?;
                }
                Err(MimeError::InvalidClassifierInput {
                    reason: "inspection failed".to_owned(),
                })
            },
        )
        .expect_err("inspection must fail");
        let path = recorded.expect("inspection path");
        let sandbox = path.parent().expect("sandbox path");
        assert!(!path.exists(), "explicit cleanup must remove the payload");
        if block_cleanup {
            fs::remove_file(sandbox.join("retained")).expect("remove test blocker");
            fs::remove_dir(sandbox).expect("remove residual sandbox");
            match error {
                MimeError::TemporaryCleanup { primary, cleanup } => {
                    assert!(
                        matches!(*primary, MimeError::InvalidClassifierInput { ref reason } if reason == "inspection failed")
                    );
                    assert_eq!(cleanup.operation(), LocalFileOperation::Cleanup);
                    assert_eq!(cleanup.path(), Some(sandbox));
                }
                other => panic!("primary and cleanup errors must remain available: {other}"),
            }
        } else {
            assert!(!sandbox.exists());
            assert!(matches!(error, MimeError::InvalidClassifierInput { ref reason } if reason == "inspection failed"));
        }
    }
}
