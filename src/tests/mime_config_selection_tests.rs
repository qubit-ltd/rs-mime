// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Unit tests for configured MIME provider selections.

use qubit_spi::ProviderSelectionTargetRef;

use crate::MimeConfig;

#[test]
fn test_default_media_stream_classifier_selection_is_accessible() {
    let config = MimeConfig::default();

    assert!(matches!(
        config.media_stream_classifier_selection().target(),
        ProviderSelectionTargetRef::Named(selector) if selector.as_str() == "ffprobe"
    ));
}
