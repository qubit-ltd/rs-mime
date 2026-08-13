// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_mime::MimeConfig;
use qubit_spi::{ProviderMetadata, ServiceProvider};

use crate::support::{TestMimeDetectorProvider, TestProviderBehavior};

#[test]
fn test_mime_detector_provider_defaults_and_factory() {
    let provider = TestMimeDetectorProvider::new(
        "static",
        7,
        TestProviderBehavior::Success("application/x-static"),
    )
    .with_aliases(&["static-alias"]);
    let descriptor = provider.descriptor();
    let detector = provider
        .create_configured(&MimeConfig::default())
        .expect("static provider should create detector");

    assert_eq!("static", descriptor.id().as_str());
    assert_eq!("static-alias", descriptor.aliases()[0].as_str());
    assert_eq!(7, descriptor.priority());
    assert_eq!(
        Some("application/x-static".to_owned()),
        detector
            .detect_by_filename("sample.static")
            .expect("filename detection should succeed"),
    );
}
