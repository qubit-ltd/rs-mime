/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Integration tests using real files imported from Java `common-mime` fixtures.

use std::fs::File;
use std::io::{
    Read,
    Seek,
};
use std::path::{
    Path,
    PathBuf,
};

use qubit_mime::{
    MimeDetectionPolicy,
    MimeRepository,
    MimeType,
    RepositoryMimeDetector,
};

/// Directory containing the real file fixtures.
const FIXTURE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/real_files");

/// Describes expected repository detection results for a real fixture.
#[derive(Debug, Clone, Copy)]
struct RealFileCase {
    /// Fixture filename.
    filename: &'static str,
    /// Expected filename-only candidates.
    by_filename: &'static [&'static str],
    /// Expected content-only candidates.
    by_content: &'static [&'static str],
    /// Expected detector result for `detect_file` with verified content.
    by_detector: &'static str,
}

/// Real-file cases mirrored from Java `MimeRepositoryTest.TEST_DATA`.
const REAL_FILE_CASES: &[RealFileCase] = &[
    RealFileCase {
        filename: "test.PNG",
        by_filename: &["image/png"],
        by_content: &["image/png"],
        by_detector: "image/png",
    },
    RealFileCase {
        filename: "test.jpg",
        by_filename: &["image/jpeg"],
        by_content: &["image/jpeg"],
        by_detector: "image/jpeg",
    },
    RealFileCase {
        filename: "test.gif",
        by_filename: &["image/gif"],
        by_content: &["image/gif"],
        by_detector: "image/gif",
    },
    RealFileCase {
        filename: "test.pdf",
        by_filename: &["application/pdf"],
        by_content: &["application/pdf"],
        by_detector: "application/pdf",
    },
    RealFileCase {
        filename: "test.html",
        by_filename: &["text/html"],
        by_content: &["text/html"],
        by_detector: "text/html",
    },
    RealFileCase {
        filename: "test.txt",
        by_filename: &["text/plain"],
        by_content: &[],
        by_detector: "text/plain",
    },
    RealFileCase {
        filename: "test.mp3",
        by_filename: &["audio/mpeg"],
        by_content: &["audio/mpeg"],
        by_detector: "audio/mpeg",
    },
    RealFileCase {
        filename: "test.ogg",
        by_filename: &[
            "audio/ogg",
            "video/ogg",
            "audio/x-vorbis+ogg",
            "audio/x-flac+ogg",
            "audio/x-speex+ogg",
            "video/x-theora+ogg",
        ],
        by_content: &["audio/x-vorbis+ogg"],
        by_detector: "audio/x-vorbis+ogg",
    },
    RealFileCase {
        filename: "test.rtf",
        by_filename: &["application/rtf"],
        by_content: &["application/rtf"],
        by_detector: "application/rtf",
    },
    RealFileCase {
        filename: "test.doc",
        by_filename: &["application/msword"],
        by_content: &["application/msword"],
        by_detector: "application/msword",
    },
];

/// Builds an absolute fixture path.
///
/// # Parameters
/// - `filename`: Fixture filename.
///
/// # Returns
/// Absolute fixture path in this crate.
fn fixture_path(filename: &str) -> PathBuf {
    Path::new(FIXTURE_DIR).join(filename)
}

/// Reads the meaningful content prefix for repository magic matching.
///
/// # Parameters
/// - `repository`: Repository whose maximum magic byte count determines the read size.
/// - `path`: Fixture path to read.
///
/// # Returns
/// Content prefix read from the fixture.
fn read_magic_prefix(repository: &MimeRepository, path: &Path) -> Vec<u8> {
    let mut file = File::open(path).expect("real fixture should open");
    let mut buffer = vec![0; repository.max_test_bytes()];
    let bytes_read = file
        .read(&mut buffer)
        .expect("real fixture prefix should be readable");
    buffer.truncate(bytes_read);
    buffer
}

/// Converts MIME type references to owned names.
///
/// # Parameters
/// - `mime_types`: MIME type references returned by the repository.
///
/// # Returns
/// MIME type names in detection order.
fn mime_names(mime_types: Vec<&MimeType>) -> Vec<String> {
    mime_types
        .into_iter()
        .map(|mime_type| mime_type.name().to_owned())
        .collect()
}

/// Converts expected names to owned strings.
///
/// # Parameters
/// - `expected`: Expected MIME names.
///
/// # Returns
/// Owned MIME names for assertion.
fn expected_names(expected: &[&str]) -> Vec<String> {
    expected.iter().map(|name| (*name).to_owned()).collect()
}

#[test]
fn test_real_files_match_java_repository_filename_and_magic_expectations() {
    let detector = RepositoryMimeDetector::new().expect("default repository should load");
    let repository = detector.repository();

    for case in REAL_FILE_CASES {
        let path = fixture_path(case.filename);
        assert!(path.exists(), "fixture should exist: {}", path.display());
        let prefix = read_magic_prefix(repository, &path);

        assert_eq!(
            expected_names(case.by_filename),
            mime_names(repository.detect_by_filename(path.to_string_lossy().as_ref())),
            "filename detection mismatch for {}",
            case.filename,
        );
        assert_eq!(
            expected_names(case.by_content),
            mime_names(repository.detect_by_content(&prefix)),
            "content detection mismatch for {}",
            case.filename,
        );
    }
}

#[test]
fn test_repository_detector_detects_real_files_from_paths() {
    let detector = RepositoryMimeDetector::new().expect("default repository should load");

    for case in REAL_FILE_CASES {
        let path = fixture_path(case.filename);
        let detected = detector
            .detect_file(&path, MimeDetectionPolicy::VerifyContent)
            .expect("real fixture should be detectable");

        assert_eq!(
            Some(case.by_detector.to_owned()),
            detected,
            "path detection mismatch for {}",
            case.filename,
        );
    }
}

#[test]
fn test_repository_detector_detects_real_files_from_readers_without_consuming_position() {
    let detector = RepositoryMimeDetector::new().expect("default repository should load");
    let reader_cases = [
        ("test.PNG", "image/png"),
        ("test.pdf", "application/pdf"),
        ("test.ogg", "audio/x-vorbis+ogg"),
    ];

    for (filename, expected) in reader_cases {
        let path = fixture_path(filename);
        let mut file = File::open(&path).expect("real fixture should open");

        let detected = detector
            .detect_reader(
                &mut file,
                Some(path.to_string_lossy().as_ref()),
                MimeDetectionPolicy::VerifyContent,
            )
            .expect("reader detection should succeed");

        assert_eq!(
            Some(expected.to_owned()),
            detected,
            "reader detection mismatch for {filename}",
        );
        assert_eq!(
            0,
            file.stream_position()
                .expect("fixture reader position should be available"),
            "reader position should be restored for {filename}",
        );
    }
}
