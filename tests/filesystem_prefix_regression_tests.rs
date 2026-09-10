// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Detection consumes a bounded prefix through exactly one filesystem reader.

#[allow(dead_code)]
#[path = "support/prefix_file_system_spi.rs"]
mod prefix_file_system_spi;
use prefix_file_system_spi::PrefixFileSystemSpi;
use qubit_fs::FileSystem;
use qubit_fs::Path;
use qubit_mime::MimeDetectionPolicy;
use qubit_mime::MimeDetector;
use qubit_mime::MimeError;
use qubit_mime::RepositoryMimeDetector;

/// Files larger than the probe remain eligible, with no stat or surplus reads.
#[test]
fn test_detection_prefix_counts_for_oversized_resources() {
    let detector = RepositoryMimeDetector::new().expect("repository detector");
    let detector: &dyn MimeDetector = &detector;
    let maximum = 16;
    for size in [maximum + 1, 100_000] {
        for range in [false, true] {
            let mut content = b"%PDF-1.7\n".to_vec();
            content.resize(size, b' ');
            let provider = PrefixFileSystemSpi::hierarchical(content);
            let provider = if range {
                provider.with_guaranteed_range()
            } else {
                provider
            };
            let filesystem = FileSystem::from_spi(provider.clone()).expect("filesystem");
            let detected = detector
                .detect_path(
                    &filesystem,
                    &Path::parse("/document.bin").expect("path"),
                    maximum,
                    MimeDetectionPolicy::VerifyContent,
                )
                .expect("detection");
            assert_eq!(detected.as_deref(), Some("application/pdf"));
            assert_eq!(provider.opened(), 1);
            assert_eq!(provider.stat_calls(), 0);
            assert!(provider.requested_read_bytes() <= maximum);
        }
    }
}

/// Detector admission rejects an excessive limit before opening the provider.
#[test]
fn test_detection_buffer_limit_precedes_filesystem_open() {
    let detector = RepositoryMimeDetector::new().expect("repository detector");
    let detector: &dyn MimeDetector = &detector;
    let provider = PrefixFileSystemSpi::hierarchical(b"%PDF-1.7\n".to_vec());
    let filesystem = FileSystem::from_spi(provider.clone()).expect("filesystem");
    let maximum = detector
        .max_buffer_size()
        .checked_add(1)
        .expect("finite detector limit");
    let error = detector
        .detect_path(
            &filesystem,
            &Path::parse("/document.pdf").expect("path"),
            maximum,
            MimeDetectionPolicy::VerifyContent,
        )
        .expect_err("buffer limit");
    assert!(matches!(error, MimeError::BufferLimitExceeded { .. }));
    assert_eq!(provider.opened(), 0);
    assert_eq!(provider.stat_calls(), 0);
    assert_eq!(provider.requested_read_bytes(), 0);
}
