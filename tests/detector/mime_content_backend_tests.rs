// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Content adapters preserve reader position and declared input bounds.

use crate::support::DirectBackendDetector;
use std::io::Cursor;

use qubit_mime::ContentRequirement;
use qubit_mime::MimeContentBackend;
use qubit_mime::MimeDetectorAdapter;
use qubit_mime::MimeDetectorBackend;
use qubit_mime::MimeResult;
use qubit_mime::RepositoryMimeDetector;

/// The legacy adapter retains backend limits and detects from a nonzero reader
/// position.
#[test]
fn test_detector_adapter_uses_backend_prefix_and_restores_position() {
    let backend = RepositoryMimeDetector::new().expect("built-in repository");
    let expected_limit = backend.max_test_bytes();
    let adapter = MimeDetectorAdapter::new(backend);
    assert_eq!(adapter.backend().max_test_bytes(), expected_limit);
    assert_eq!(
        adapter.content_requirement(),
        ContentRequirement::Prefix(expected_limit)
    );
    let mut reader = Cursor::new(b"skip%PDF-1.7\n".to_vec());
    reader.set_position(4);
    let candidates = adapter.detect_reader(&mut reader).expect("detect PDF prefix");
    assert!(candidates.iter().any(|candidate| candidate == "application/pdf"));
    assert_eq!(reader.position(), 4);
}

#[derive(Debug)]
struct LengthBackend(ContentRequirement);

impl MimeContentBackend for LengthBackend {
    fn content_requirement(&self) -> ContentRequirement {
        self.0
    }

    fn detect_bytes(&self, bytes: &[u8]) -> MimeResult<Vec<String>> {
        Ok(vec![bytes.len().to_string()])
    }
}

/// Complete and bounded content contracts consume the intended span and rewind
/// on success.
#[test]
fn test_content_reader_obeys_prefix_and_complete_requirements() {
    for (requirement, expected) in [
        (ContentRequirement::Prefix(2), "2"),
        (ContentRequirement::Prefix(0), "0"),
        (ContentRequirement::Complete, "5"),
    ] {
        let mut reader = Cursor::new(b"skip12345".to_vec());
        reader.set_position(4);
        let backend = LengthBackend(requirement);
        assert_eq!(
            backend.detect_reader(&mut reader).expect("read selected content"),
            vec![expected]
        );
        assert_eq!(reader.position(), 4);
    }
}

#[test]
fn test_adapter_limits_input_and_restores_reader_position() {
    let adapter = MimeDetectorAdapter::new(DirectBackendDetector::new());
    assert_eq!(adapter.backend().max_test_bytes(), 5);
    assert_eq!(adapter.content_requirement(), ContentRequirement::Prefix(5));
    let mut reader = Cursor::new(b"xxhello ignored trailer".to_vec());
    reader.set_position(2);
    assert_eq!(
        adapter.detect_reader(&mut reader).expect("prefix must be classified"),
        ["text/plain"]
    );
    assert_eq!(reader.position(), 2);
    assert!(
        adapter
            .detect_bytes(b"different")
            .expect("nonmatching input is valid")
            .is_empty()
    );
}
