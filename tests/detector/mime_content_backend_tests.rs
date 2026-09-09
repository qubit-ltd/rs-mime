// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Verifies content adapters retain prefix and reader-position semantics.
use std::io::Cursor;

use qubit_mime::ContentRequirement;
use qubit_mime::MimeContentBackend;
use qubit_mime::MimeDetectorAdapter;
use qubit_mime::MimeDetectorBackend;

use crate::support::DirectBackendDetector;

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
