// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#![cfg(feature = "inventory")]

use std::sync::Arc;

use qubit_mime::MimeConfig;
use qubit_mime::MimeDetector;
use qubit_mime::MimeDetectorSpec;
use qubit_mime::MimeError;
use qubit_spi::ProviderDescriptor;
use qubit_spi::ProviderMetadata;
use qubit_spi::ServiceProvider;
use qubit_spi::error::ProviderFailure;
use qubit_spi::provider_descriptor;

struct FirstDuplicateSelectorProvider;

struct SecondDuplicateSelectorProvider;

impl ProviderMetadata for FirstDuplicateSelectorProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        provider_descriptor!("duplicate-selector")
    }
}
impl ProviderMetadata for SecondDuplicateSelectorProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        provider_descriptor!("duplicate-selector")
    }
}
impl ServiceProvider<MimeDetectorSpec> for FirstDuplicateSelectorProvider {
    fn create_configured(&self, _: &MimeConfig) -> Result<Arc<dyn MimeDetector>, ProviderFailure<MimeError>> {
        unimplemented!()
    }
}

impl ServiceProvider<MimeDetectorSpec> for SecondDuplicateSelectorProvider {
    fn create_configured(&self, _: &MimeConfig) -> Result<Arc<dyn MimeDetector>, ProviderFailure<MimeError>> {
        unimplemented!()
    }
}

qubit_spi::submit_sync_provider! {
    inventory_entry = qubit_mime::detector::mime_detector_inventory::Entry;
    spec = MimeDetectorSpec;
    provider = FirstDuplicateSelectorProvider;
}
qubit_spi::submit_sync_provider! {
    inventory_entry = qubit_mime::detector::mime_detector_inventory::Entry;
    spec = MimeDetectorSpec;
    provider = SecondDuplicateSelectorProvider;
}

#[test]
fn test_duplicate_selector_reports_submitted_source() {
    let error = qubit_mime::detector::mime_detector_inventory::build_registry()
        .expect_err("duplicate selector must fail inventory construction");
    let message = error.to_string();
    assert!(message.contains("duplicate-selector"), "{message}");
    assert!(message.contains("inventory_duplicate_tests.rs"), "{message}");
}
