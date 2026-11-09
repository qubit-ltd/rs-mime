// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::Arc;

use crate::MimeRepository;

#[test]
fn test_bundled_repository_single_allocation() {
    let first = MimeRepository::bundled_shared();
    let second = MimeRepository::bundled_shared();

    assert!(Arc::ptr_eq(&first, &second));
    assert!(std::ptr::eq(MimeRepository::bundled(), first.as_ref()));
    assert!(first.get("application/pdf").is_some());
}
